//! The text tokens print for enum-like values, and small derived values (sortable title,
//! decade, channel layout).

use cv_domain::{AiredStatus, Edition, Genre, MediaSource};

/// Leading words that move to the end of a sortable title. Apostrophe forms attach to the next
/// word (`L'Avare`), the others need a space.
pub const DEFAULT_TITLE_PREFIXES: &[&str] = &[
    "A", "An", "The", "Der", "Die", "Das", "Ein", "Eine", "Le", "La", "Les", "L'", "L\u{b4}", "L`",
    "Un", "Une", "Des", "Du", "D'", "D\u{b4}", "D`",
];

/// The title with a leading article moved to the end: `The Matrix` becomes `Matrix, The`.
/// The match ignores case and keeps the title's own spelling of the prefix. A title that is
/// only the prefix stays as it is.
pub fn sortable_title(title: &str, prefixes: &[String]) -> String {
    let title = title.trim();
    for prefix in prefixes {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            continue;
        }
        let joined = prefix.ends_with(['\'', '\u{b4}', '`']);
        let Some(head) = title.get(..prefix.len()) else {
            continue;
        };
        if !head.eq_ignore_ascii_case(prefix) {
            continue;
        }
        let rest = &title[prefix.len()..];
        let rest = if joined {
            rest
        } else if let Some(after_space) = rest.strip_prefix(' ') {
            after_space
        } else {
            continue;
        };
        let rest = rest.trim_start();
        if !rest.is_empty() {
            return format!("{rest}, {head}");
        }
    }
    title.to_owned()
}

/// `SCIENCE_FICTION` as `Science Fiction`; `TV` stays upper case.
fn title_case_key(key: &str) -> String {
    key.split('_')
        .map(|word| match word {
            "TV" => "TV".to_owned(),
            _ => {
                let mut chars = word.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                })
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The English name of a genre. A genre outside the built-in set prints as the user wrote it.
pub fn genre_label(genre: &Genre) -> String {
    match genre {
        Genre::Other(text) => text.clone(),
        known => title_case_key(known.name()),
    }
}

/// The name of an edition; no edition prints nothing.
pub fn edition_label(edition: &Edition) -> String {
    match edition {
        Edition::None => String::new(),
        Edition::DirectorsCut => "Director's Cut".to_owned(),
        Edition::CollectorsEdition => "Collector's Edition".to_owned(),
        Edition::Imax => "IMAX".to_owned(),
        Edition::Other(text) => text.clone(),
        known => title_case_key(known.name()),
    }
}

/// The name of a media source as it appears in file names; an unknown source prints nothing.
pub fn media_source_label(source: &MediaSource) -> String {
    let label = match source {
        MediaSource::UhdBluray => "UHD Blu-ray",
        MediaSource::Bluray => "Blu-ray",
        MediaSource::Dvd => "DVD",
        MediaSource::HdDvd => "HD-DVD",
        MediaSource::Tv => "TV",
        MediaSource::Vhs => "VHS",
        MediaSource::Laserdisc => "LaserDisc",
        MediaSource::DVhs => "D-VHS",
        MediaSource::HdRip => "HDRip",
        MediaSource::Cam => "CAM",
        MediaSource::Ts => "TS",
        MediaSource::Tc => "TC",
        MediaSource::DvdScr => "DVDSCR",
        MediaSource::R5 => "R5",
        MediaSource::WebRip => "WebRip",
        MediaSource::WebDl => "Web-DL",
        MediaSource::Stream => "Stream",
        MediaSource::Unknown => "",
        MediaSource::Other(text) => text.as_str(),
    };
    label.to_owned()
}

/// The airing status of a show; unknown prints nothing.
pub fn status_label(status: AiredStatus) -> &'static str {
    match status {
        AiredStatus::Unknown => "",
        AiredStatus::Continuing => "Continuing",
        AiredStatus::Ended => "Ended",
        AiredStatus::Canceled => "Canceled",
    }
}

/// The channel layout in dot notation: 1 is `1.0`, 2 is `2.0`, 6 is `5.1`, 8 is `7.1`. Three
/// channels are `2.1`, four and five have no LFE (`4.0`, `5.0`), and from six up one channel
/// is the LFE. Zero channels print nothing.
pub fn audio_channels_dot(channels: u32) -> String {
    match channels {
        0 => String::new(),
        1 | 2 | 4 | 5 => format!("{channels}.0"),
        n => format!("{}.1", n - 1),
    }
}

/// `1990-1999` for a year in the nineties; nothing for year 0.
pub fn decade_long(year: u16) -> String {
    if year == 0 {
        return String::new();
    }
    let start = year / 10 * 10;
    format!("{start}-{}", start + 9)
}

/// `1990s` for a year in the nineties; nothing for year 0.
pub fn decade_short(year: u16) -> String {
    if year == 0 {
        return String::new();
    }
    format!("{}s", year / 10 * 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefixes() -> Vec<String> {
        DEFAULT_TITLE_PREFIXES
            .iter()
            .map(|p| (*p).to_owned())
            .collect()
    }

    #[test]
    fn sortable_titles() {
        let p = prefixes();
        assert_eq!(sortable_title("The Matrix", &p), "Matrix, The");
        assert_eq!(sortable_title("the matrix", &p), "matrix, the");
        assert_eq!(
            sortable_title("The Last Lighthouse", &p),
            "Last Lighthouse, The"
        );
        assert_eq!(sortable_title("A Quiet Place", &p), "Quiet Place, A");
        assert_eq!(sortable_title("L'Avare", &p), "Avare, L'");
        assert_eq!(
            sortable_title("Theory of Everything", &p),
            "Theory of Everything"
        );
        assert_eq!(sortable_title("The", &p), "The");
        assert_eq!(sortable_title("Dune: Part Two", &p), "Dune: Part Two");
        assert_eq!(sortable_title("", &p), "");
    }

    #[test]
    fn labels() {
        assert_eq!(genre_label(&Genre::ScienceFiction), "Science Fiction");
        assert_eq!(genre_label(&Genre::RealityTv), "Reality TV");
        assert_eq!(
            genre_label(&Genre::Other("Mockumentary".into())),
            "Mockumentary"
        );
        assert_eq!(edition_label(&Edition::ExtendedEdition), "Extended Edition");
        assert_eq!(edition_label(&Edition::DirectorsCut), "Director's Cut");
        assert_eq!(edition_label(&Edition::None), "");
        assert_eq!(edition_label(&Edition::Imax), "IMAX");
        assert_eq!(media_source_label(&MediaSource::WebDl), "Web-DL");
        assert_eq!(media_source_label(&MediaSource::Bluray), "Blu-ray");
        assert_eq!(media_source_label(&MediaSource::UhdBluray), "UHD Blu-ray");
        assert_eq!(media_source_label(&MediaSource::Unknown), "");
        assert_eq!(status_label(AiredStatus::Continuing), "Continuing");
    }

    #[test]
    fn every_known_value_has_a_label_or_is_none() {
        for genre in Genre::known() {
            assert!(!genre_label(&genre).is_empty());
        }
        for edition in Edition::known() {
            assert_eq!(edition_label(&edition).is_empty(), edition == Edition::None);
        }
        for source in MediaSource::known() {
            assert_eq!(
                media_source_label(&source).is_empty(),
                source == MediaSource::Unknown
            );
        }
    }

    #[test]
    fn channel_layouts_and_decades() {
        let dots: Vec<String> = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12]
            .iter()
            .map(|n| audio_channels_dot(*n))
            .collect();
        assert_eq!(
            dots,
            [
                "", "1.0", "2.0", "2.1", "4.0", "5.0", "5.1", "6.1", "7.1", "11.1"
            ]
        );
        assert_eq!(decade_long(1999), "1990-1999");
        assert_eq!(decade_short(2024), "2020s");
        assert_eq!(decade_long(0), "");
        assert_eq!(decade_short(0), "");
    }
}
