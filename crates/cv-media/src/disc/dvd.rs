//! DVD-Video: finding the main title in the `.IFO` files.
//!
//! `VIDEO_TS.IFO` lists the titles (`TT_SRPT`): for each, the title set (`VTS_nn`) and the
//! title's number inside it. `VTS_nn_0.IFO` maps that number to a program chain (`VTS_PTT_SRPT`
//! and `VTS_PGCIT`), and the chain's header carries its playback time. The main title is the
//! one with the longest playback time. All numbers are big-endian; sectors are 2048 bytes.

use super::files::{DiscFs, find, join, read_file};
use super::{DiscError, DiscKind, MainTitle};

const SECTOR: usize = 2048;
/// The IFO files of real discs are well under a megabyte.
const IFO_LIMIT: u64 = 8 * 1024 * 1024;

/// A title as the video manager lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TitleEntry {
    /// 1-based position in the disc's title list.
    pub number: u16,
    pub chapters: u16,
    /// The title set (`VTS_nn`).
    pub title_set: u8,
    /// The title's number inside its set.
    pub title_in_set: u8,
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

/// The table `pointer_at` points to (a sector number), or `None` when the pointer is empty.
fn table(ifo: &[u8], pointer_at: usize) -> Option<&[u8]> {
    let sector = usize::try_from(be32(ifo, pointer_at)?).ok()?;
    if sector == 0 {
        return None;
    }
    ifo.get(sector.checked_mul(SECTOR)?..)
}

/// The titles of `VIDEO_TS.IFO`.
///
/// # Errors
/// [`DiscError::Structure`] when the file is not a video manager or its table is cut short.
pub fn parse_titles(ifo: &[u8]) -> Result<Vec<TitleEntry>, DiscError> {
    if !ifo.starts_with(b"DVDVIDEO-VMG") {
        return Err(DiscError::structure(
            "VIDEO_TS.IFO is not a DVD video manager",
        ));
    }
    let cut = || DiscError::structure("VIDEO_TS.IFO is cut short");
    let table = table(ifo, 0xC4).ok_or_else(cut)?;
    let count = usize::from(be16(table, 0).ok_or_else(cut)?);
    let mut titles = Vec::with_capacity(count);
    for index in 0..count {
        let at = 8 + index * 12;
        let entry = table.get(at..at + 12).ok_or_else(cut)?;
        titles.push(TitleEntry {
            number: u16::try_from(index + 1).unwrap_or(u16::MAX),
            chapters: be16(entry, 2).ok_or_else(cut)?,
            title_set: entry[6],
            title_in_set: entry[7],
        });
    }
    Ok(titles)
}

/// A DVD time (`hh mm ss ff` in BCD, the top two bits of the last byte give the frame rate)
/// in milliseconds.
#[must_use]
pub fn playback_ms(time: [u8; 4]) -> u64 {
    let bcd = |byte: u8| u64::from(byte >> 4) * 10 + u64::from(byte & 0x0F);
    let seconds = bcd(time[0]) * 3600 + bcd(time[1]) * 60 + bcd(time[2]);
    let frames = bcd(time[3] & 0x3F);
    let frame_ms = match time[3] >> 6 {
        1 => frames * 1000 / 25,
        3 => frames * 1001 / 30,
        _ => 0,
    };
    seconds * 1000 + frame_ms
}

/// The program chains of one title set: `(playback time in ms)` for each, by 1-based number.
fn chain_durations(ifo: &[u8]) -> Result<Vec<u64>, DiscError> {
    if !ifo.starts_with(b"DVDVIDEO-VTS") {
        return Err(DiscError::structure(
            "a VTS_nn_0.IFO is not a DVD title set",
        ));
    }
    let cut = || DiscError::structure("a VTS_nn_0.IFO is cut short");
    let chains = table(ifo, 0xCC).ok_or_else(cut)?;
    let count = usize::from(be16(chains, 0).ok_or_else(cut)?);
    let mut durations = Vec::with_capacity(count);
    for index in 0..count {
        let search = chains
            .get(8 + index * 8..8 + index * 8 + 8)
            .ok_or_else(cut)?;
        let offset = usize::try_from(be32(search, 4).ok_or_else(cut)?).map_err(|_| cut())?;
        let time = chains.get(offset + 4..offset + 8).ok_or_else(cut)?;
        durations.push(playback_ms([time[0], time[1], time[2], time[3]]));
    }
    Ok(durations)
}

/// The chain (1-based) that title `title_in_set` of a title set plays, and its chapter count.
fn title_chain(ifo: &[u8], title_in_set: u8) -> Option<(usize, usize)> {
    let ptt = table(ifo, 0xC8)?;
    let count = usize::from(be16(ptt, 0)?);
    let last_byte = usize::try_from(be32(ptt, 4)?).ok()?;
    let index = usize::from(title_in_set).checked_sub(1)?;
    if index >= count {
        return None;
    }
    let start = usize::try_from(be32(ptt, 8 + index * 4)?).ok()?;
    let end = if index + 1 < count {
        usize::try_from(be32(ptt, 8 + (index + 1) * 4)?).ok()?
    } else {
        last_byte + 1
    };
    let chapters = end.checked_sub(start)? / 4;
    let chain = usize::from(be16(ptt, start)?);
    (chain > 0).then_some((chain, chapters))
}

/// The playback time and chapter count of a title, from its title set's IFO.
///
/// # Errors
/// [`DiscError::Structure`] when the IFO does not describe the title.
pub fn title_playback(ifo: &[u8], title_in_set: u8) -> Result<(u64, usize), DiscError> {
    let durations = chain_durations(ifo)?;
    let (chain, chapters) = title_chain(ifo, title_in_set)
        .ok_or_else(|| DiscError::structure("a DVD title has no program chain"))?;
    let duration = durations
        .get(chain - 1)
        .copied()
        .ok_or_else(|| DiscError::structure("a DVD title names a missing program chain"))?;
    Ok((duration, chapters))
}

fn vts_name(set: u8, part: &str) -> String {
    format!("VTS_{set:02}_{part}")
}

/// Finds the main title of the DVD whose `VIDEO_TS` folder is `video_ts` (a path inside `fs`).
///
/// # Errors
/// [`DiscError::Structure`] when no title can be read; I/O errors of the disc.
pub fn find_main_title(fs: &dyn DiscFs, video_ts: &str) -> Result<MainTitle, DiscError> {
    let mut best: Option<(u64, u16, u8, usize, String)> = None;
    let mut consider = |duration: u64, number: u16, set: u8, chapters: usize, source: String| {
        if best.as_ref().is_none_or(|current| duration > current.0) {
            best = Some((duration, number, set, chapters, source));
        }
    };

    let titles = match find(fs, video_ts, "VIDEO_TS.IFO")? {
        Some(entry) => parse_titles(&read_file(fs, &join(video_ts, &entry.name), IFO_LIMIT)?).ok(),
        None => None,
    };
    match titles {
        Some(titles) if !titles.is_empty() => {
            for title in titles {
                let name = vts_name(title.title_set, "0.IFO");
                let Some(entry) = find(fs, video_ts, &name)? else {
                    continue;
                };
                let ifo = read_file(fs, &join(video_ts, &entry.name), IFO_LIMIT)?;
                if let Ok((duration, chapters)) = title_playback(&ifo, title.title_in_set) {
                    let chapters = if chapters == 0 {
                        usize::from(title.chapters)
                    } else {
                        chapters
                    };
                    consider(duration, title.number, title.title_set, chapters, name);
                }
            }
        }
        // No usable video manager: take the longest chain of any title set.
        _ => {
            for set in 1..=99_u8 {
                let name = vts_name(set, "0.IFO");
                let Some(entry) = find(fs, video_ts, &name)? else {
                    continue;
                };
                let ifo = read_file(fs, &join(video_ts, &entry.name), IFO_LIMIT)?;
                if let Ok(durations) = chain_durations(&ifo) {
                    for (index, duration) in durations.into_iter().enumerate() {
                        let number = u16::try_from(index + 1).unwrap_or(u16::MAX);
                        consider(duration, number, set, 0, name.clone());
                    }
                }
            }
        }
    }

    let (duration, number, set, chapters, source) =
        best.ok_or_else(|| DiscError::structure("no DVD title could be read from the IFO files"))?;

    // The title set's video: VTS_nn_1.VOB, _2, ... in order.
    let entries = fs.list(video_ts)?;
    let mut files: Vec<(String, u64)> = (1..=9)
        .filter_map(|part| {
            let name = vts_name(set, &format!("{part}.VOB"));
            entries
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(&name))
                .map(|entry| (join(video_ts, &entry.name), entry.size))
        })
        .collect();
    files.retain(|(_, size)| *size > 0);
    let stream_file = files
        .first()
        .map(|(path, _)| path.clone())
        .ok_or_else(|| DiscError::structure(format!("title set {set} has no VOB files")))?;
    Ok(MainTitle {
        kind: DiscKind::Dvd,
        source: format!("{source} (title {number})"),
        duration_ms: Some(duration),
        chapters: u32::try_from(chapters).unwrap_or(u32::MAX),
        stream_file,
        files: files.into_iter().map(|(path, _)| path).collect(),
    })
}
