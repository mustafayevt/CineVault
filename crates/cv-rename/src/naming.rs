//! File-name rules for artwork, trailers, thumbnails and NFOs.
//!
//! A naming profile lists, for each kind of file, the choices that apply (`FILENAME_POSTER`,
//! `POSTER`, ...). Every choice gives one file name; all of them get the same content. A
//! choice is a [`NameRule`]: the name is built from the base name of the video, a fixed stem, or
//! a stem inside a folder. Choices this version does not know are ignored.
//!
//! Movies in a shared folder may only use the rules
//! that build on the video name ([`NameRule::Base`]), because a fixed name such as `poster.jpg`
//! would be shared by every movie of the folder. Movies on a disc structure prefer the fixed
//! names.

/// How one naming choice builds a file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRule {
    /// `<base><suffix>.<ext>`: `-poster` gives `Heat (1995)-poster.jpg`.
    Base(&'static str),
    /// `<stem>.<ext>`: `poster` gives `poster.jpg`.
    Fixed(&'static str),
    /// `<dir>/<stem>.<ext>`: `extrafanart` and `fanart{N}` give `extrafanart/fanart1.jpg`.
    InFolder {
        dir: &'static str,
        stem: &'static str,
    },
    /// `<dir>/<base><suffix>.<ext>`: `trailers` and `-trailer` give
    /// `trailers/Heat (1995)-trailer.mp4`.
    InFolderBase {
        dir: &'static str,
        suffix: &'static str,
    },
}

impl NameRule {
    /// The file name relative to the movie folder. `index` replaces `{N}` (extra fanart).
    pub fn file_name(&self, base: &str, ext: &str, index: Option<u32>) -> String {
        let number = index.map(|n| n.to_string()).unwrap_or_default();
        let with_ext = |stem: String| {
            if ext.is_empty() {
                stem
            } else {
                format!("{stem}.{ext}")
            }
        };
        match self {
            NameRule::Base(suffix) => with_ext(format!("{base}{}", suffix.replace("{N}", &number))),
            NameRule::Fixed(stem) => with_ext(stem.replace("{N}", &number)),
            NameRule::InFolder { dir, stem } => {
                format!("{dir}/{}", with_ext(stem.replace("{N}", &number)))
            }
            NameRule::InFolderBase { dir, suffix } => {
                format!(
                    "{dir}/{}",
                    with_ext(format!("{base}{}", suffix.replace("{N}", &number)))
                )
            }
        }
    }

    /// True when the name is built from the video name, so movies of one folder do not collide.
    pub fn is_per_movie(&self) -> bool {
        matches!(self, NameRule::Base(_) | NameRule::InFolderBase { .. })
    }

    /// The fixed-name form a disc structure prefers: `-poster` becomes `poster`. Rules that are
    /// fixed already stay as they are.
    pub fn disc_form(&self) -> NameRule {
        match self {
            NameRule::Base(suffix) => {
                let stem = suffix.trim_start_matches(['-', '.']);
                if stem.is_empty() {
                    *self
                } else {
                    NameRule::Fixed(stem)
                }
            }
            other => *other,
        }
    }
}

/// The kinds of file with several naming choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtKind {
    Poster,
    Fanart,
    ExtraFanart,
    Banner,
    Clearart,
    Thumb,
    Clearlogo,
    Discart,
    Keyart,
    Squareart,
    Trailer,
}

impl ArtKind {
    /// The `FILENAME_*` choice that is always safe in a shared folder.
    pub fn default_per_movie(self) -> NameRule {
        match self {
            ArtKind::Poster => NameRule::Base("-poster"),
            ArtKind::Fanart => NameRule::Base("-fanart"),
            ArtKind::ExtraFanart => NameRule::Base("-fanart{N}"),
            ArtKind::Banner => NameRule::Base("-banner"),
            ArtKind::Clearart => NameRule::Base("-clearart"),
            ArtKind::Thumb => NameRule::Base("-landscape"),
            ArtKind::Clearlogo => NameRule::Base("-clearlogo"),
            ArtKind::Discart => NameRule::Base("-discart"),
            ArtKind::Keyart => NameRule::Base("-keyart"),
            ArtKind::Squareart => NameRule::Base("-squareart"),
            ArtKind::Trailer => NameRule::Base("-trailer"),
        }
    }
}

/// The rule of a movie naming choice, or `None` for a name this version does not know.
pub fn movie_rule(kind: ArtKind, choice: &str) -> Option<NameRule> {
    let choice = choice.trim().to_ascii_uppercase();
    let choice = choice.as_str();
    use NameRule::{Base, Fixed, InFolder, InFolderBase};
    Some(match (kind, choice) {
        (ArtKind::Poster, "FILENAME") => Base(""),
        (ArtKind::Poster, "FILENAME_POSTER") => Base("-poster"),
        (ArtKind::Poster, "MOVIE") => Fixed("movie"),
        (ArtKind::Poster, "POSTER") => Fixed("poster"),
        (ArtKind::Poster, "FOLDER") => Fixed("folder"),
        (ArtKind::Poster, "COVER") => Fixed("cover"),

        (ArtKind::Fanart, "FILENAME_FANART") => Base("-fanart"),
        (ArtKind::Fanart, "FILENAME_FANART2") => Base(".fanart"),
        (ArtKind::Fanart, "FANART") => Fixed("fanart"),
        (ArtKind::Fanart, "FILENAME_BACKDROP") => Base("-backdrop"),
        (ArtKind::Fanart, "FILENAME_BACKDROP2") => Base(".backdrop"),
        (ArtKind::Fanart, "BACKDROP") => Fixed("backdrop"),
        (ArtKind::Fanart, "FILENAME_BACKGROUND") => Base("-background"),
        (ArtKind::Fanart, "FILENAME_BACKGROUND2") => Base(".background"),
        (ArtKind::Fanart, "BACKGROUND") => Fixed("background"),

        (ArtKind::ExtraFanart, "FILENAME_EXTRAFANART") => Base("-fanart{N}"),
        (ArtKind::ExtraFanart, "FILENAME_EXTRAFANART2") => Base(".fanart{N}"),
        (ArtKind::ExtraFanart, "EXTRAFANART") => Fixed("fanart{N}"),
        (ArtKind::ExtraFanart, "FOLDER_EXTRAFANART") => InFolder {
            dir: "extrafanart",
            stem: "fanart{N}",
        },
        (ArtKind::ExtraFanart, "FILENAME_EXTRABACKDROP") => Base("-backdrop{N}"),
        (ArtKind::ExtraFanart, "FILENAME_EXTRABACKDROP2") => Base(".backdrop{N}"),
        (ArtKind::ExtraFanart, "EXTRABACKDROP") => Fixed("backdrop{N}"),
        (ArtKind::ExtraFanart, "FOLDER_EXTRABACKDROP") => InFolder {
            dir: "extrafanart",
            stem: "backdrop{N}",
        },
        (ArtKind::ExtraFanart, "FILENAME_EXTRABACKGROUND") => Base("-background{N}"),
        (ArtKind::ExtraFanart, "FILENAME_EXTRABACKGROUND2") => Base(".background{N}"),
        (ArtKind::ExtraFanart, "EXTRABACKGROUND") => Fixed("background{N}"),
        (ArtKind::ExtraFanart, "FOLDER_EXTRABACKGROUND") => InFolder {
            dir: "extrafanart",
            stem: "background{N}",
        },

        (ArtKind::Banner, "FILENAME_BANNER") => Base("-banner"),
        (ArtKind::Banner, "BANNER") => Fixed("banner"),

        (ArtKind::Clearart, "FILENAME_CLEARART") => Base("-clearart"),
        (ArtKind::Clearart, "CLEARART") => Fixed("clearart"),

        (ArtKind::Thumb, "FILENAME_THUMB") => Base("-thumb"),
        (ArtKind::Thumb, "FILENAME_LANDSCAPE") => Base("-landscape"),
        (ArtKind::Thumb, "THUMB") => Fixed("thumb"),
        (ArtKind::Thumb, "LANDSCAPE") => Fixed("landscape"),

        (ArtKind::Clearlogo, "FILENAME_CLEARLOGO") => Base("-clearlogo"),
        (ArtKind::Clearlogo, "CLEARLOGO") => Fixed("clearlogo"),
        (ArtKind::Clearlogo, "FILENAME_LOGO") => Base("-logo"),
        (ArtKind::Clearlogo, "LOGO") => Fixed("logo"),

        (ArtKind::Discart, "FILENAME_DISC") => Base("-disc"),
        (ArtKind::Discart, "DISC") => Fixed("disc"),
        (ArtKind::Discart, "FILENAME_DISCART") => Base("-discart"),
        (ArtKind::Discart, "DISCART") => Fixed("discart"),

        (ArtKind::Keyart, "FILENAME_KEYART") => Base("-keyart"),
        (ArtKind::Keyart, "KEYART") => Fixed("keyart"),

        (ArtKind::Squareart, "FILENAME_SQUAREART") => Base("-squareart"),
        (ArtKind::Squareart, "SQUAREART") => Fixed("squareart"),
        (ArtKind::Squareart, "FILENAME_SQUARE") => Base("-square"),
        (ArtKind::Squareart, "SQUARE") => Fixed("square"),
        (ArtKind::Squareart, "BACKGROUND_SQUARE") => Fixed("backgroundSquare"),

        (ArtKind::Trailer, "FILENAME_TRAILER") => Base("-trailer"),
        (ArtKind::Trailer, "MOVIE_TRAILER") => Fixed("movie-trailer"),
        (ArtKind::Trailer, "TRAILERS_FILENAME_TRAILER") => InFolderBase {
            dir: "trailers",
            suffix: "-trailer",
        },
        (ArtKind::Trailer, "TRAILER") => Fixed("trailer"),
        _ => return None,
    })
}

/// The movie rules of a list of choices, in order and without duplicates. Unknown choices are
/// skipped.
pub fn movie_rules(kind: ArtKind, choices: &[String]) -> Vec<NameRule> {
    let mut rules: Vec<NameRule> = Vec::new();
    for choice in choices {
        if let Some(rule) = movie_rule(kind, choice)
            && !rules.contains(&rule)
        {
            rules.push(rule);
        }
    }
    rules
}

/// The rules for a movie in a shared folder: only the ones built on the video name, and the
/// `FILENAME_*` default when the profile has none of them.
pub fn shared_folder_rules(kind: ArtKind, rules: &[NameRule]) -> Vec<NameRule> {
    let per_movie: Vec<NameRule> = rules
        .iter()
        .copied()
        .filter(NameRule::is_per_movie)
        .collect();
    if per_movie.is_empty() && !rules.is_empty() {
        vec![kind.default_per_movie()]
    } else {
        per_movie
    }
}

/// The rules for a movie on a disc structure: fixed names where the profile has a `FILENAME_*`
/// choice.
pub fn disc_rules(rules: &[NameRule]) -> Vec<NameRule> {
    let mut out: Vec<NameRule> = Vec::new();
    for rule in rules {
        let rule = rule.disc_form();
        if !out.contains(&rule) {
            out.push(rule);
        }
    }
    out
}

// ---- TV --------------------------------------------------------------------------------------

/// The kinds of show-level artwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShowArt {
    Poster,
    Fanart,
    Banner,
    Clearart,
    Clearlogo,
    Discart,
    Characterart,
    Keyart,
    Thumb,
    Squareart,
}

/// The stem of a show artwork choice (`POSTER` gives `poster`), or `None` when unknown.
pub fn show_stem(kind: ShowArt, choice: &str) -> Option<&'static str> {
    let choice = choice.trim().to_ascii_uppercase();
    Some(match (kind, choice.as_str()) {
        (ShowArt::Poster, "POSTER") => "poster",
        (ShowArt::Poster, "FOLDER") => "folder",
        (ShowArt::Poster, "COVER") => "cover",
        (ShowArt::Fanart, "FANART") => "fanart",
        (ShowArt::Fanart, "BACKDROP") => "backdrop",
        (ShowArt::Fanart, "BACKGROUND") => "background",
        (ShowArt::Banner, "BANNER") => "banner",
        (ShowArt::Clearart, "CLEARART") => "clearart",
        (ShowArt::Clearlogo, "CLEARLOGO") => "clearlogo",
        (ShowArt::Clearlogo, "LOGO") => "logo",
        (ShowArt::Discart, "DISC") => "disc",
        (ShowArt::Discart, "DISCART") => "discart",
        (ShowArt::Characterart, "CHARACTERART") => "characterart",
        (ShowArt::Keyart, "KEYART") => "keyart",
        (ShowArt::Thumb, "THUMB") => "thumb",
        (ShowArt::Thumb, "LANDSCAPE") => "landscape",
        (ShowArt::Squareart, "SQUAREART") => "squareart",
        (ShowArt::Squareart, "SQUARE") => "square",
        (ShowArt::Squareart, "BACKGROUND_SQUARE") => "backgroundSquare",
        _ => return None,
    })
}

/// The kinds of season artwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeasonArt {
    Poster,
    Fanart,
    Banner,
    Thumb,
}

impl SeasonArt {
    fn suffix(self) -> &'static str {
        match self {
            SeasonArt::Poster => "-poster",
            SeasonArt::Fanart => "-fanart",
            SeasonArt::Banner => "-banner",
            SeasonArt::Thumb => "-thumb",
        }
    }

    fn setting(self) -> &'static str {
        match self {
            SeasonArt::Poster => "SEASON_POSTER",
            SeasonArt::Fanart => "SEASON_FANART",
            SeasonArt::Banner => "SEASON_BANNER",
            SeasonArt::Thumb => "SEASON_THUMB",
        }
    }
}

/// The base of a season's artwork names: `season01`, `season-specials` or `season-all`.
pub fn season_stem(season: i64) -> String {
    match season {
        0 => "season-specials".to_owned(),
        n if n < 0 => "season-all".to_owned(),
        n => format!("season{n:02}"),
    }
}

/// The path of a season artwork file relative to the show folder.
///
/// `SEASON_POSTER` (and `SEASON_FANART`, ...) put `season01-poster.jpg` in the show folder,
/// `SEASON_FOLDER` puts it in the season folder. `FOLDER`, `POSTER` and `COVER` (posters only)
/// name the file after the folder style (`Season 1/folder.jpg`). With no season folder the file
/// stays in the show folder. `None` for a choice this version does not know.
pub fn season_art_path(
    kind: SeasonArt,
    choice: &str,
    season: i64,
    season_folder: &str,
    ext: &str,
) -> Option<String> {
    let choice = choice.trim().to_ascii_uppercase();
    let in_folder = |name: String| {
        if season_folder.is_empty() {
            name
        } else {
            format!("{season_folder}/{name}")
        }
    };
    let file = |stem: String| format!("{stem}.{ext}");
    let styled = format!("{}{}", season_stem(season), kind.suffix());
    if choice == kind.setting() {
        return Some(file(styled));
    }
    match (kind, choice.as_str()) {
        (_, "SEASON_FOLDER") => Some(in_folder(file(styled))),
        (SeasonArt::Poster, "FOLDER") => Some(in_folder(file("folder".to_owned()))),
        (SeasonArt::Poster, "POSTER") => Some(in_folder(file("poster".to_owned()))),
        (SeasonArt::Poster, "COVER") => Some(in_folder(file("cover".to_owned()))),
        _ => None,
    }
}

/// The path of an episode thumbnail relative to the folder of the episode, for the choices
/// `FILENAME` (`<base>.jpg`), `FILENAME_THUMB` (`<base>-thumb.jpg`) and `FILENAME_TBN`
/// (`<base>.tbn`).
pub fn episode_thumb_name(choice: &str, base: &str, ext: &str) -> Option<String> {
    match choice.trim().to_ascii_uppercase().as_str() {
        "FILENAME" => Some(format!("{base}.{ext}")),
        "FILENAME_THUMB" => Some(format!("{base}-thumb.{ext}")),
        "FILENAME_TBN" => Some(format!("{base}.tbn")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_choices_build_names() {
        let name = |kind, choice: &str, ext| {
            movie_rule(kind, choice)
                .expect("known choice")
                .file_name("Heat (1995)", ext, None)
        };
        assert_eq!(
            name(ArtKind::Poster, "FILENAME_POSTER", "jpg"),
            "Heat (1995)-poster.jpg"
        );
        assert_eq!(name(ArtKind::Poster, "filename", "png"), "Heat (1995).png");
        assert_eq!(name(ArtKind::Poster, "POSTER", "jpg"), "poster.jpg");
        assert_eq!(
            name(ArtKind::Fanart, "FILENAME_FANART2", "jpg"),
            "Heat (1995).fanart.jpg"
        );
        assert_eq!(
            name(ArtKind::Squareart, "BACKGROUND_SQUARE", "jpg"),
            "backgroundSquare.jpg"
        );
        assert_eq!(
            name(ArtKind::Discart, "FILENAME_DISC", "png"),
            "Heat (1995)-disc.png"
        );
        assert_eq!(
            name(ArtKind::Trailer, "MOVIE_TRAILER", "mp4"),
            "movie-trailer.mp4"
        );
        assert_eq!(
            name(ArtKind::Trailer, "TRAILERS_FILENAME_TRAILER", "mp4"),
            "trailers/Heat (1995)-trailer.mp4"
        );
        assert!(movie_rule(ArtKind::Poster, "NOPE").is_none());
        assert!(movie_rule(ArtKind::Banner, "POSTER").is_none());
    }

    #[test]
    fn extra_fanart_choices_take_an_index() {
        let rule = movie_rule(ArtKind::ExtraFanart, "FOLDER_EXTRAFANART").unwrap();
        assert_eq!(
            rule.file_name("Heat", "jpg", Some(3)),
            "extrafanart/fanart3.jpg"
        );
        let rule = movie_rule(ArtKind::ExtraFanart, "FILENAME_EXTRAFANART").unwrap();
        assert_eq!(rule.file_name("Heat", "jpg", Some(1)), "Heat-fanart1.jpg");
        let rule = movie_rule(ArtKind::ExtraFanart, "EXTRABACKDROP").unwrap();
        assert_eq!(rule.file_name("Heat", "jpg", Some(2)), "backdrop2.jpg");
    }

    #[test]
    fn rules_drop_duplicates_and_unknown_choices() {
        let choices = vec![
            "FILENAME_POSTER".to_owned(),
            "BOGUS".to_owned(),
            "filename_poster".to_owned(),
            "POSTER".to_owned(),
        ];
        assert_eq!(
            movie_rules(ArtKind::Poster, &choices),
            [NameRule::Base("-poster"), NameRule::Fixed("poster")]
        );
    }

    #[test]
    fn shared_folders_keep_only_names_built_on_the_video() {
        let rules = [NameRule::Fixed("poster"), NameRule::Base("-poster")];
        assert_eq!(
            shared_folder_rules(ArtKind::Poster, &rules),
            [NameRule::Base("-poster")]
        );
        assert_eq!(
            shared_folder_rules(ArtKind::Poster, &[NameRule::Fixed("poster")]),
            [NameRule::Base("-poster")]
        );
        assert!(shared_folder_rules(ArtKind::Poster, &[]).is_empty());
    }

    #[test]
    fn discs_prefer_fixed_names() {
        assert_eq!(
            disc_rules(&[NameRule::Base("-poster"), NameRule::Fixed("poster")]),
            [NameRule::Fixed("poster")]
        );
        assert_eq!(
            disc_rules(&[NameRule::Base("-fanart"), NameRule::Base(".fanart")]),
            [NameRule::Fixed("fanart")]
        );
        assert_eq!(disc_rules(&[NameRule::Base("")]), [NameRule::Base("")]);
    }

    #[test]
    fn season_artwork_paths() {
        let path = |kind, choice, season, folder| {
            season_art_path(kind, choice, season, folder, "jpg").unwrap()
        };
        assert_eq!(
            path(SeasonArt::Poster, "SEASON_POSTER", 1, "Season 1"),
            "season01-poster.jpg"
        );
        assert_eq!(
            path(SeasonArt::Poster, "SEASON_FOLDER", 1, "Season 1"),
            "Season 1/season01-poster.jpg"
        );
        assert_eq!(
            path(SeasonArt::Poster, "SEASON_FOLDER", 1, ""),
            "season01-poster.jpg"
        );
        assert_eq!(
            path(SeasonArt::Fanart, "SEASON_FOLDER", 0, "Specials"),
            "Specials/season-specials-fanart.jpg"
        );
        assert_eq!(
            path(SeasonArt::Banner, "SEASON_BANNER", -1, ""),
            "season-all-banner.jpg"
        );
        assert_eq!(
            path(SeasonArt::Poster, "folder", 2, "Season 2"),
            "Season 2/folder.jpg"
        );
        assert!(season_art_path(SeasonArt::Fanart, "SEASON_POSTER", 1, "", "jpg").is_none());
    }

    #[test]
    fn episode_thumbnails() {
        assert_eq!(episode_thumb_name("FILENAME", "E", "jpg").unwrap(), "E.jpg");
        assert_eq!(
            episode_thumb_name("FILENAME_THUMB", "E", "png").unwrap(),
            "E-thumb.png"
        );
        assert_eq!(
            episode_thumb_name("FILENAME_TBN", "E", "jpg").unwrap(),
            "E.tbn"
        );
        assert!(episode_thumb_name("x", "E", "jpg").is_none());
    }

    #[test]
    fn show_stems() {
        assert_eq!(show_stem(ShowArt::Poster, "poster"), Some("poster"));
        assert_eq!(
            show_stem(ShowArt::Squareart, "BACKGROUND_SQUARE"),
            Some("backgroundSquare")
        );
        assert_eq!(show_stem(ShowArt::Fanart, "POSTER"), None);
    }
}
