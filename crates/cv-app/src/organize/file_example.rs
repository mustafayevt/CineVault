//! The live example of the first run's Renaming step.
//!
//! The first run has no library yet, so nothing can be previewed. This renders a profile's movie
//! templates on one video: the one the folder scan picked (with what its NFO says, such as the
//! TMDB ID a media-server style adds), or a built-in sample.

use std::path::Path;
use std::sync::LazyLock;

use cv_domain::{AudioStream, Item, ItemDetail, MediaFile, MediaFileType};
use cv_rename::{MovieRenamer, RenameProfile};
use cv_scan::parse::{
    detect_clean_title_and_year, detect_edition, detect_ids, detect_media_source,
};
use regex::Regex;

/// The file name the first-run example uses when the folder scan has found no video yet.
pub const SAMPLE_FILE: &str = "Dune.Part.Two.2024.2160p.WEB-DL.DV.HDR10.TrueHD.Atmos.7.1.mkv";

/// A movie file name rendered under a profile's templates: what it is now and what it would be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileExample {
    /// The file name the example starts from.
    pub current: String,
    /// The folder the profile gives.
    pub folder: String,
    /// The file name the profile gives, with the extension.
    pub file: String,
    /// The title read from the name.
    pub title: String,
    pub year: u16,
    /// `current` is the built-in sample, not a file of the user.
    pub sample: bool,
}

static RESOLUTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[^a-z0-9])(2160p|4k|uhd|1080[pi]|720p|576p|480p)(?:[^a-z0-9]|$)")
        .expect("resolution pattern is valid")
});
static CHANNELS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^0-9])([1-9])\.([01])(?:[^0-9]|$)").expect("channels pattern is valid")
});

/// The picture size a release name states, as width and height.
fn size_in(name: &str) -> Option<(u32, u32)> {
    let found = RESOLUTION.captures(name)?[1].to_lowercase();
    Some(match found.as_str() {
        "2160p" | "4k" | "uhd" => (3840, 2160),
        "1080p" | "1080i" => (1920, 1080),
        "720p" => (1280, 720),
        "576p" => (720, 576),
        _ => (720, 480),
    })
}

/// The HDR formats a release name states.
fn hdr_in(name: &str) -> Vec<String> {
    let lower = name.to_lowercase();
    let has = |token: &str| {
        lower
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '+')
            .any(|part| part == token)
    };
    let mut found = Vec::new();
    if has("dv") || lower.contains("dolby.vision") || lower.contains("dolby vision") {
        found.push("Dolby Vision".to_owned());
    }
    if has("hdr10+") {
        found.push("HDR10+".to_owned());
    } else if has("hdr10") || has("hdr") {
        found.push("HDR10".to_owned());
    }
    found
}

/// The audio codec and channel count a release name states.
fn audio_in(name: &str) -> Option<AudioStream> {
    let lower = name.to_lowercase();
    let codec = if lower.contains("truehd") && lower.contains("atmos") {
        "TrueHD Atmos"
    } else if lower.contains("truehd") {
        "TrueHD"
    } else if lower.contains("dts-hd") || lower.contains("dts.hd") || lower.contains("dtshd") {
        "DTS-HD MA"
    } else if lower.contains("dts") {
        "DTS"
    } else if lower.contains("ddp") || lower.contains("eac3") || lower.contains("dd+") {
        "E-AC-3"
    } else if lower.contains("ac3") || lower.contains("dd5") || lower.contains("dd2") {
        "AC3"
    } else {
        return None;
    };
    let channels = CHANNELS
        .captures(name)
        .and_then(|c| Some(c[1].parse::<u32>().ok()? + c[2].parse::<u32>().ok()?))
        .unwrap_or(2);
    Some(AudioStream {
        codec: codec.to_owned(),
        language: "en".to_owned(),
        channels,
        ..AudioStream::default()
    })
}

/// Reads a movie file name the way the scanner does (title, year, edition, source and provider
/// ID) plus the picture and sound the release tags state, as a movie to render.
fn item_of(file_name: &str) -> Item {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    let clean = detect_clean_title_and_year::<&str>(stem, &[]);
    let mut item = Item::movie(&clean.title);
    item.core.year = clean.year.unwrap_or(0);
    if let Some(tmdb) = detect_ids(stem).tmdb {
        item.core.ids.set("tmdb", tmdb.to_string());
    }
    let mut video = MediaFile::new(MediaFileType::Video, "example", file_name);
    if let Some((width, height)) = size_in(stem) {
        video.width = width;
        video.height = height;
    }
    video.hdr_formats = hdr_in(stem);
    video.audio_streams.extend(audio_in(stem));
    item.core.media_files.push(video);
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.media_source = detect_media_source(stem);
        fields.edition = detect_edition(stem);
    }
    item
}

/// How many release tags a file name states (resolution, source, HDR, sound): the first run's
/// example uses the video that says the most, so the naming styles differ on it.
#[must_use]
pub fn tag_count(file_name: &str) -> usize {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    usize::from(size_in(stem).is_some())
        + usize::from(detect_media_source(stem) != cv_domain::MediaSource::Unknown)
        + usize::from(!hdr_in(stem).is_empty())
        + usize::from(audio_in(stem).is_some())
}

/// What the NFO next to a video adds to the example: its title, year and provider IDs. The NFO
/// is `<video name>.nfo` or `movie.nfo` in the video's folder; one that cannot be read adds
/// nothing.
fn add_nfo(item: &mut Item, video: &Path) {
    let Some(dir) = video.parent() else {
        return;
    };
    let named = video.with_extension("nfo");
    let Some(text) = [named, dir.join("movie.nfo")]
        .iter()
        .find_map(|path| std::fs::read(path).ok())
    else {
        return;
    };
    let Some(nfo) = cv_nfo::read_movie::read_movie_nfo(&cv_nfo::read::decode_nfo_bytes(&text))
    else {
        return;
    };
    if nfo.is_valid() {
        item.core.title.clone_from(&nfo.title);
        if let Ok(year) = u16::try_from(nfo.year)
            && year > 0
        {
            item.core.year = year;
        }
    }
    for (key, value) in nfo.ids.iter() {
        item.core.ids.set(key, value);
    }
}

/// Renders `profile`'s movie templates on `file_name` (the built-in sample when `None` or blank).
/// A full path of a video that exists also reads the NFO beside it. `None` when the title cannot
/// be read from the name or a template does not work.
pub fn example_for_file(profile: &RenameProfile, file_name: Option<&str>) -> Option<FileExample> {
    let given = file_name.map(str::trim).filter(|name| !name.is_empty());
    let video = given
        .map(Path::new)
        .filter(|path| path.is_absolute() && path.is_file());
    let current = match video {
        Some(path) => path.file_name()?.to_str()?,
        None => given.unwrap_or(SAMPLE_FILE),
    };
    let mut item = item_of(current);
    if let Some(path) = video {
        add_nfo(&mut item, path);
    }
    if item.core.title.trim().is_empty() {
        return None;
    }
    let renamer = MovieRenamer::new(profile.movie.clone());
    let folder = renamer.folder_name(&item, None).ok()?;
    let base = renamer.file_name(&item, None).ok()?;
    let file = match current.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() && ext.len() <= 5 => format!("{base}.{ext}"),
        _ => base,
    };
    Some(FileExample {
        current: current.to_owned(),
        folder,
        file,
        title: item.core.title.clone(),
        year: item.core.year,
        sample: given.is_none(),
    })
}

#[cfg(test)]
mod tests {
    use cv_rename::Preset;

    use super::*;

    fn profile(preset: Preset) -> RenameProfile {
        RenameProfile::from_preset(preset)
    }

    #[test]
    fn a_found_file_is_renamed_under_the_clean_templates() {
        let found = example_for_file(
            &profile(Preset::Clean),
            Some("Heat.1995.1080p.BluRay.x264-GRP.mkv"),
        )
        .unwrap();
        assert!(!found.sample);
        assert_eq!(found.current, "Heat.1995.1080p.BluRay.x264-GRP.mkv");
        assert_eq!(found.folder, "Heat (1995)");
        assert_eq!(found.file, "Heat (1995).mkv");
    }

    #[test]
    fn the_detailed_templates_use_the_tags_of_the_name() {
        let found = example_for_file(
            &profile(Preset::Detailed),
            Some("Heat.1995.1080p.BluRay.DTS.5.1.mkv"),
        )
        .unwrap();
        assert!(
            found.folder.starts_with("Heat (1995) [1080p]"),
            "{}",
            found.folder
        );
        assert!(found.file.ends_with(".mkv"));
    }

    #[test]
    fn a_video_on_disk_takes_the_ids_of_its_nfo_so_a_media_server_style_adds_them() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Alien (1979)");
        std::fs::create_dir_all(&folder).unwrap();
        let video = folder.join("Alien (1979).mkv");
        std::fs::write(&video, b"x").unwrap();
        std::fs::write(
            folder.join("Alien (1979).nfo"),
            "<movie><title>Alien</title><year>1979</year>\
             <uniqueid type=\"tmdb\" default=\"true\">348</uniqueid></movie>",
        )
        .unwrap();
        let path = video.to_string_lossy().into_owned();
        let safe = example_for_file(&profile(Preset::MediaServerSafe), Some(&path)).unwrap();
        assert_eq!(safe.current, "Alien (1979).mkv");
        assert!(!safe.sample);
        assert!(safe.folder.contains("348"), "{}", safe.folder);
        let clean = example_for_file(&profile(Preset::Clean), Some(&path)).unwrap();
        assert_eq!(clean.folder, "Alien (1979)");
        assert_ne!(safe.folder, clean.folder);
    }

    #[test]
    fn the_tags_of_a_name_are_counted() {
        assert_eq!(tag_count("Alien (1979).mkv"), 0);
        assert_eq!(tag_count("Heat.1995.1080p.BluRay.DTS.5.1.mkv"), 3);
        assert_eq!(tag_count(SAMPLE_FILE), 4);
    }

    #[test]
    fn without_a_file_the_built_in_sample_is_used_and_said_so() {
        let found = example_for_file(&profile(Preset::Clean), None).unwrap();
        assert!(found.sample);
        assert_eq!(found.current, SAMPLE_FILE);
        assert!(found.folder.ends_with("(2024)"), "{}", found.folder);
        let blank = example_for_file(&profile(Preset::Clean), Some("  ")).unwrap();
        assert!(blank.sample);
    }

    #[test]
    fn the_media_server_safe_templates_add_the_id_only_when_the_name_has_one() {
        let with = example_for_file(
            &profile(Preset::MediaServerSafe),
            Some("Heat (1995) tmdbid-949.mkv"),
        )
        .unwrap();
        assert_eq!(with.folder, "Heat (1995) {tmdb-949}");
        let without = example_for_file(
            &profile(Preset::MediaServerSafe),
            Some("Heat.1995.1080p.mkv"),
        )
        .unwrap();
        assert_eq!(without.folder, "Heat (1995)");
    }
}
