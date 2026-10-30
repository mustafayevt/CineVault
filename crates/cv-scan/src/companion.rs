//! Which video a file belongs to by its name, in a folder several titles share.
//!
//! A Scan gives the files of a folder with several movies, and of a season folder with several
//! episodes, to the video whose base name the file's name equals or continues after a space,
//! dot, comma, underscore or hyphen (`Movie.en.srt`, `Movie-poster.jpg`, `Movie.cd1.mkv`), the
//! longest base name first, so `Alien.Resurrection.en.srt` is not mistaken for a file of
//! `Alien.mkv`. The walkers use these rules, and so does a delete that takes the files named
//! after a title (spec `title-deletion`). Names are compared without regard to case. Nothing is
//! read from disk.

use cv_domain::MediaFileType;

use crate::classify::classify;
use crate::stacking::{parse_file_name, parse_folder_name};

/// The name without its last extension. A leading dot is not an extension.
pub fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

/// The video's base name without extension and without a trailing part marker, so all parts of
/// a stack give the same text. An empty base name becomes `empty`.
pub fn normalized_base(file_name: &str) -> String {
    let without_marker = parse_file_name(file_name).cleaned;
    let folder_form = parse_folder_name(stem(&without_marker)).cleaned;
    let trimmed = folder_form
        .trim_end_matches([' ', '.', '_', '-'])
        .trim_start()
        .to_owned();
    if trimmed.is_empty() {
        "empty".to_owned()
    } else {
        trimmed
    }
}

/// `stem` equals the movie's base name or continues it after a space, dot, comma, underscore or
/// hyphen (`Movie-poster`, `Movie.en`, `Movie.cd1`). Both are lower case.
pub fn is_sibling(stem: &str, base: &str) -> bool {
    stem == base
        || stem
            .strip_prefix(base)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_whitespace() || matches!(c, '.' | ',' | '_' | '-'))
}

/// A video by its name: `classify` calls it a video and it is not part of a disc structure. A
/// trailer or sample named after a movie (`Movie-trailer.mkv`) is a companion, not a video.
pub fn is_plain_video(file_name: &str) -> bool {
    let found = classify(file_name);
    found.file_type == MediaFileType::Video && !found.disc_file
}

/// The lower-case base names a companion of the video may continue: its stem and, when it
/// differs, the stem without a part marker (`movie.cd1` and `movie`).
pub fn video_bases(file_name: &str) -> Vec<String> {
    let raw = stem(file_name).to_lowercase();
    let base = normalized_base(file_name).to_lowercase();
    if base == raw {
        vec![raw]
    } else {
        vec![raw, base]
    }
}

/// Who a file belongs to by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameOwner<K> {
    /// No base name matches.
    Nobody,
    /// The owner of the longest matching base name.
    One(K),
    /// Different owners hold the longest matching base name.
    Ambiguous,
}

/// The owner of `file_name` among `(lower-case base name, owner)` pairs: the owner of the longest
/// base name the file's stem equals or continues ([`is_sibling`]). When that longest base name
/// belongs to more than one owner, the answer is [`NameOwner::Ambiguous`].
pub fn owner_by_name<K: Copy + PartialEq>(file_name: &str, bases: &[(String, K)]) -> NameOwner<K> {
    let own = stem(file_name).to_lowercase();
    let mut best: Option<(usize, K)> = None;
    let mut tied = false;
    for (base, owner) in bases {
        if !is_sibling(&own, base) {
            continue;
        }
        let length = base.chars().count();
        match best {
            Some((longest, _)) if length < longest => {}
            Some((longest, current)) if length == longest => {
                tied |= current != *owner;
            }
            _ => {
                best = Some((length, *owner));
                tied = false;
            }
        }
    }
    match best {
        None => NameOwner::Nobody,
        Some(_) if tied => NameOwner::Ambiguous,
        Some((_, owner)) => NameOwner::One(owner),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bases(videos: &[(&str, u8)]) -> Vec<(String, u8)> {
        videos
            .iter()
            .flat_map(|(name, owner)| video_bases(name).into_iter().map(|b| (b, *owner)))
            .collect()
    }

    #[test]
    fn sidecars_of_every_kind_belong_to_their_video() {
        let list = bases(&[("Example Movie (2020).mkv", 1)]);
        for name in [
            "Example Movie (2020).mkv",
            "Example Movie (2020).nfo",
            "Example Movie (2020).en.srt",
            "Example Movie (2020).en.forced.srt",
            "Example Movie (2020).en.sdh.srt",
            "Example Movie (2020).sup",
            "Example Movie (2020).ass",
            "Example Movie (2020).idx",
            "Example Movie (2020).sub",
            "Example Movie (2020)-poster.jpg",
            "Example Movie (2020)-fanart.jpg",
            "Example Movie (2020)-clearlogo.png",
            "Example Movie (2020).jpg",
            "Example Movie (2020)-trailer.mp4",
            "Example Movie (2020).chapters.xml",
            "Example Movie (2020)_alt.srt",
            "example movie (2020).EN.SRT",
        ] {
            assert_eq!(owner_by_name(name, &list), NameOwner::One(1), "{name}");
        }
        for name in [
            "poster.jpg",
            "Example Movie (2020)x.srt",
            "Example Movie.srt",
            "Other.nfo",
        ] {
            assert_eq!(owner_by_name(name, &list), NameOwner::Nobody, "{name}");
        }
    }

    #[test]
    fn the_longest_base_name_wins() {
        let list = bases(&[
            ("Alien (1979).mkv", 1),
            ("Alien (1979) Director's Cut.mkv", 2),
            ("Heat.mkv", 3),
            ("Heat 2.mkv", 4),
            ("Movie.mkv", 5),
            ("Movie.1080p.mkv", 6),
        ]);
        let cases = [
            ("Alien (1979).en.srt", 1),
            ("Alien (1979)-poster.jpg", 1),
            ("Alien (1979) Director's Cut.en.srt", 2),
            ("Alien (1979) Director's Cut.mkv", 2),
            ("Heat.en.srt", 3),
            ("Heat 2.en.srt", 4),
            ("Heat 2-poster.jpg", 4),
            ("Movie.en.srt", 5),
            ("Movie.1080p.en.srt", 6),
            ("Movie.1080p.nfo", 6),
        ];
        for (name, owner) in cases {
            assert_eq!(owner_by_name(name, &list), NameOwner::One(owner), "{name}");
        }
    }

    #[test]
    fn the_same_base_name_of_two_owners_is_ambiguous() {
        let list = bases(&[("Movie.mkv", 1), ("Movie.avi", 2)]);
        assert_eq!(owner_by_name("Movie.en.srt", &list), NameOwner::Ambiguous);
        // A longer base name of one owner still decides.
        let list = bases(&[("Movie.mkv", 1), ("Movie.avi", 2), ("Movie.1080p.mkv", 1)]);
        assert_eq!(
            owner_by_name("Movie.1080p.en.srt", &list),
            NameOwner::One(1)
        );
        // Two videos of the same owner are not a tie.
        let list = bases(&[("Movie CD1.mkv", 1), ("Movie CD2.mkv", 1)]);
        assert_eq!(owner_by_name("Movie.en.srt", &list), NameOwner::One(1));
    }

    #[test]
    fn trailers_and_samples_are_not_videos() {
        assert!(is_plain_video("Movie.mkv"));
        assert!(!is_plain_video("Movie-trailer.mkv"));
        assert!(!is_plain_video("Movie-sample.mkv"));
        assert!(!is_plain_video("Movie.en.srt"));
        assert!(!is_plain_video("VIDEO_TS.VOB"));
    }

    #[test]
    fn part_markers_give_the_base_name_too() {
        assert_eq!(video_bases("Movie CD1.mkv"), ["movie cd1", "movie"]);
        assert_eq!(video_bases("Heat.mkv"), ["heat"]);
    }
}
