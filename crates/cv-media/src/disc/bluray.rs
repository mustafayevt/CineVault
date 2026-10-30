//! Blu-ray: finding the main playlist in `BDMV/PLAYLIST/*.mpls`.
//!
//! A movie playlist is a list of play items, each a time range of one clip (`STREAM/nnnnn.m2ts`)
//! on a 45 kHz clock. The main title is the playlist with the longest total playing time.
//! All numbers are big-endian.

use super::files::{DiscFs, find, join, read_file};
use super::{DiscError, DiscKind, MainTitle};

/// The playlist clock.
const TICKS_PER_SECOND: u64 = 45_000;
const MPLS_LIMIT: u64 = 4 * 1024 * 1024;

/// One part of a playlist: a range of a clip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayItem {
    /// The clip's five-digit name (`00001`, the file `STREAM/00001.m2ts`).
    pub clip: String,
    pub in_ticks: u32,
    pub out_ticks: u32,
}

/// A parsed `.mpls` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playlist {
    pub items: Vec<PlayItem>,
    /// Entry marks, which players show as chapters.
    pub chapters: u32,
}

impl Playlist {
    /// The total playing time, in milliseconds. A part played more than once counts once: discs
    /// carry looping playlists (one short clip played 300 times, a six-hour "title") that would
    /// otherwise outrun the film.
    #[must_use]
    pub fn duration_ms(&self) -> u64 {
        let mut seen: Vec<&PlayItem> = Vec::with_capacity(self.items.len());
        let mut ticks = 0_u64;
        for item in &self.items {
            if seen.contains(&item) {
                continue;
            }
            seen.push(item);
            ticks += u64::from(item.out_ticks.saturating_sub(item.in_ticks));
        }
        ticks * 1000 / TICKS_PER_SECOND
    }
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
    ]))
}

/// Parses the bytes of a playlist file.
///
/// # Errors
/// [`DiscError::Structure`] when the bytes are not a playlist or are cut short.
pub fn parse_playlist(bytes: &[u8]) -> Result<Playlist, DiscError> {
    let cut = || DiscError::structure("a playlist file is cut short");
    if !bytes.starts_with(b"MPLS") {
        return Err(DiscError::structure("a .mpls file has no MPLS header"));
    }
    let list_at = usize::try_from(be32(bytes, 8).ok_or_else(cut)?).map_err(|_| cut())?;
    let marks_at = usize::try_from(be32(bytes, 12).ok_or_else(cut)?).map_err(|_| cut())?;

    // PlayList(): length u32, reserved u16, play item count u16, sub path count u16.
    let item_count = usize::from(be16(bytes, list_at + 6).ok_or_else(cut)?);
    let mut at = list_at + 10;
    let mut items = Vec::with_capacity(item_count);
    for _ in 0..item_count {
        // PlayItem(): length u16, then the clip name (5 ASCII), codec id (4), flags u16,
        // reference to STC u8, in time u32, out time u32. Multi-angle items go on with the
        // other angles, which the length covers.
        let length = usize::from(be16(bytes, at).ok_or_else(cut)?);
        let body = bytes.get(at + 2..at + 2 + length).ok_or_else(cut)?;
        if body.len() < 20 {
            return Err(cut());
        }
        items.push(PlayItem {
            clip: String::from_utf8_lossy(&body[0..5]).into_owned(),
            in_ticks: be32(body, 12).ok_or_else(cut)?,
            out_ticks: be32(body, 16).ok_or_else(cut)?,
        });
        at += 2 + length;
    }

    // PlayListMark(): length u32, mark count u16, then 14 bytes per mark whose second byte is
    // the type (1 = entry mark).
    let mut chapters = 0;
    if marks_at != 0
        && let Some(count) = be16(bytes, marks_at + 4)
    {
        for index in 0..usize::from(count) {
            if bytes.get(marks_at + 6 + index * 14 + 1) == Some(&1) {
                chapters += 1;
            }
        }
    }
    Ok(Playlist { items, chapters })
}

/// Finds the longest playlist of the disc whose `BDMV` folder is `bdmv` (a path inside `fs`).
///
/// # Errors
/// [`DiscError::Structure`] when there is no readable playlist or its clip is missing; I/O
/// errors of the disc.
pub fn find_main_title(fs: &dyn DiscFs, bdmv: &str) -> Result<MainTitle, DiscError> {
    let playlist_dir = find_dir(fs, bdmv, "PLAYLIST")?;
    let stream_dir = find_dir(fs, bdmv, "STREAM")?;

    let mut best: Option<(u64, String, Playlist)> = None;
    let mut names: Vec<_> = fs
        .list(&playlist_dir)?
        .into_iter()
        .filter(|entry| !entry.is_dir && entry.name.to_ascii_lowercase().ends_with(".mpls"))
        .collect();
    names.sort_by(|a, b| a.name.cmp(&b.name));
    for entry in names {
        let Ok(bytes) = read_file(fs, &join(&playlist_dir, &entry.name), MPLS_LIMIT) else {
            continue;
        };
        let Ok(playlist) = parse_playlist(&bytes) else {
            continue;
        };
        let duration = playlist.duration_ms();
        // The first of equally long playlists wins, so the result does not depend on listing
        // order.
        if best.as_ref().is_none_or(|current| duration > current.0) {
            best = Some((duration, entry.name, playlist));
        }
    }
    let (duration, name, playlist) =
        best.ok_or_else(|| DiscError::structure("the disc has no readable Blu-ray playlist"))?;

    // The clips in play order, each once.
    let clips = fs.list(&stream_dir)?;
    let mut files: Vec<(String, u64)> = Vec::new();
    for item in &playlist.items {
        let wanted = format!("{}.m2ts", item.clip);
        let found = clips
            .iter()
            .find(|clip| clip.name.eq_ignore_ascii_case(&wanted));
        if let Some(clip) = found {
            let path = join(&stream_dir, &clip.name);
            if !files.iter().any(|(known, _)| *known == path) {
                files.push((path, clip.size));
            }
        }
    }
    // MediaInfo reads the streams from the biggest clip: the feature, not a logo or a menu.
    let stream_file = files
        .iter()
        .max_by_key(|(_, size)| *size)
        .map(|(path, _)| path.clone())
        .ok_or_else(|| DiscError::structure(format!("the clips of playlist {name} are missing")))?;
    Ok(MainTitle {
        kind: DiscKind::BluRay,
        source: name,
        duration_ms: Some(duration),
        chapters: playlist.chapters,
        stream_file,
        files: files.into_iter().map(|(path, _)| path).collect(),
    })
}

fn find_dir(fs: &dyn DiscFs, parent: &str, name: &str) -> Result<String, DiscError> {
    fs.list(parent)?
        .into_iter()
        .find(|entry| entry.is_dir && entry.name.eq_ignore_ascii_case(name))
        .map(|entry| join(parent, &entry.name))
        .ok_or_else(|| DiscError::structure(format!("the BDMV folder has no {name} folder")))
}

/// The dynamic range a clip information file states for a video stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicRange {
    Sdr,
    Hdr10,
    DolbyVision,
    /// A value the format reserves.
    Other(u8),
}

/// What a clip information file (`CLIPINF/nnnnn.clpi`) says about one elementary stream of its
/// clip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipStream {
    /// The packet id, which MediaInfo reports as the stream's `ID`.
    pub pid: u16,
    /// The stream coding type (`0x24` HEVC, `0x1b` AVC, `0x83` TrueHD, `0x90` PGS ...).
    pub coding_type: u8,
    /// The ISO 639-2 language code of an audio or subtitle stream, as stored (`eng`).
    pub language: Option<String>,
    /// The dynamic range of an HEVC video stream (Ultra HD Blu-ray).
    pub dynamic_range: Option<DynamicRange>,
    /// The HDR10+ flag of an HEVC video stream (Ultra HD Blu-ray).
    pub hdr10_plus: bool,
}

const CLPI_LIMIT: u64 = 16 * 1024 * 1024;

/// Parses the program information of a clip information file: every stream's packet id, coding
/// type, language and, for HEVC video, dynamic range and HDR10+ flag.
///
/// # Errors
/// [`DiscError::Structure`] when the bytes are not a clip information file or are cut short.
pub fn parse_clip_info(bytes: &[u8]) -> Result<Vec<ClipStream>, DiscError> {
    let cut = || DiscError::structure("a clip information file is cut short");
    if !bytes.starts_with(b"HDMV") {
        return Err(DiscError::structure("a .clpi file has no HDMV header"));
    }
    let info_at = usize::try_from(be32(bytes, 12).ok_or_else(cut)?).map_err(|_| cut())?;
    // ProgramInfo(): length u32, reserved u8, program count u8; each program: SPN start u32,
    // program map pid u16, stream count u8, group count u8, then per stream its pid u16 and a
    // StreamCodingInfo() of `length u8` bytes that starts with the coding type.
    let programs = *bytes.get(info_at + 5).ok_or_else(cut)?;
    let mut at = info_at + 6;
    let mut streams = Vec::new();
    for _ in 0..programs {
        let count = *bytes.get(at + 6).ok_or_else(cut)?;
        at += 8;
        for _ in 0..count {
            let pid = be16(bytes, at).ok_or_else(cut)?;
            let length = usize::from(*bytes.get(at + 2).ok_or_else(cut)?);
            let info = bytes.get(at + 3..at + 3 + length).ok_or_else(cut)?;
            at += 3 + length;
            let Some(&coding_type) = info.first() else {
                continue;
            };
            streams.push(clip_stream(pid, coding_type, &info[1..]));
        }
    }
    Ok(streams)
}

fn clip_stream(pid: u16, coding_type: u8, attributes: &[u8]) -> ClipStream {
    let language = |at: usize| {
        let code = attributes.get(at..at + 3)?;
        code.iter()
            .all(u8::is_ascii_alphabetic)
            .then(|| String::from_utf8_lossy(code).to_ascii_lowercase())
    };
    let mut stream = ClipStream {
        pid,
        coding_type,
        language: None,
        dynamic_range: None,
        hdr10_plus: false,
    };
    match coding_type {
        // HEVC: format and rate, aspect and flags, then dynamic range and colour space, then the
        // HDR10+ flag in the top bit.
        0x24 => {
            if let Some(&range) = attributes.get(2) {
                stream.dynamic_range = Some(match range >> 4 {
                    0 => DynamicRange::Sdr,
                    1 => DynamicRange::Hdr10,
                    2 => DynamicRange::DolbyVision,
                    other => DynamicRange::Other(other),
                });
            }
            stream.hdr10_plus = attributes.get(3).is_some_and(|flags| flags & 0x80 != 0);
        }
        // Audio: format and sampling rate, then the language.
        0x03 | 0x04 | 0x80..=0x86 | 0xa1 | 0xa2 => stream.language = language(1),
        // Presentation and interactive graphics: the language first.
        0x90 | 0x91 => stream.language = language(0),
        // Text subtitles: the character code, then the language.
        0x92 => stream.language = language(1),
        _ => {}
    }
    stream
}

/// The streams of the clip whose stream file is `stream_file` (`BDMV/STREAM/00800.m2ts`), from
/// its clip information file (`BDMV/CLIPINF/00800.clpi`, or the copy in `BDMV/BACKUP/CLIPINF`).
/// `None` when neither can be read.
#[must_use]
pub fn clip_streams(fs: &dyn DiscFs, stream_file: &str) -> Option<Vec<ClipStream>> {
    let (stream_dir, file) = stream_file.rsplit_once('/')?;
    let (bdmv, _) = stream_dir.rsplit_once('/')?;
    let (clip, _) = file.rsplit_once('.')?;
    let name = format!("{clip}.clpi");
    let backup = find_dir(fs, bdmv, "BACKUP").ok();
    [Some(bdmv.to_owned()), backup]
        .into_iter()
        .flatten()
        .filter_map(|parent| find_dir(fs, &parent, "CLIPINF").ok())
        .find_map(|dir| {
            let entry = find(fs, &dir, &name).ok()??;
            let bytes = read_file(fs, &join(&dir, &entry.name), CLPI_LIMIT).ok()?;
            parse_clip_info(&bytes).ok()
        })
}
