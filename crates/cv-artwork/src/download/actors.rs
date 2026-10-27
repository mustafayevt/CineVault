//! Actor portraits: the `.actors` folder of an item.
//!
//! Spec: `artwork` (Actor images).
//!
//! * A portrait is saved as `.actors/<name>.<ext>`: the actor's name with spaces turned into
//!   underscores and the characters `" \ : < > | / ? *` removed ([`actor_file_stem`]).
//! * A portrait whose actor is no longer in the cast is moved to the trash ([`stale_portraits`]).
//! * A folder that is a symbolic link is never written to or cleaned: it points somewhere the
//!   user arranged.
//! * Multi-movie folders get no `.actors` folder (decided by the caller, see
//!   [`crate::download::ArtworkTarget::writes_actors`]).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use cv_domain::{Credit, CreditKind};

use super::files::is_image_name;

/// The folder inside an item's folder that holds actor portraits.
pub const ACTORS_DIR: &str = ".actors";

/// Names Windows will not accept as a file name, with or without an extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// The file name (without extension) of an actor's portrait, or `None` when nothing is left of
/// the name. `Timothée Chalamet` gives `Timothée_Chalamet`.
pub fn actor_file_stem(name: &str) -> Option<String> {
    let cleaned: String = name
        .trim()
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '"' | '\\' | ':' | '<' | '>' | '|' | '/' | '?' | '*')
        })
        .map(|c| if c == ' ' { '_' } else { c })
        .collect();
    // A name cannot end in a dot on Windows, and `..` must never reach the file system.
    let cleaned = cleaned.trim_end_matches(['.', '_']).trim_start_matches('.');
    if cleaned.is_empty() {
        return None;
    }
    let first = cleaned.split('.').next().unwrap_or(cleaned);
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(first)) {
        return Some(format!("{cleaned}_"));
    }
    Some(cleaned.to_owned())
}

/// An actor of the cast, with the address of the portrait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantedActor {
    pub name: String,
    /// The file name without extension.
    pub stem: String,
    /// The remembered source of the portrait; empty when the provider had none.
    pub url: String,
}

/// The actors of a cast that get a portrait file, in cast order. Only actors count (crew and
/// guests do not). Two names that clean up to the same file name share it: the first wins.
pub fn wanted_actors(cast: &[Credit]) -> Vec<WantedActor> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for credit in cast.iter().filter(|c| c.kind == CreditKind::Actor) {
        let Some(stem) = actor_file_stem(&credit.person.name) else {
            continue;
        };
        if !seen.insert(stem.to_lowercase()) {
            continue;
        }
        out.push(WantedActor {
            name: credit.person.name.clone(),
            stem,
            url: credit.person.thumb_url.trim().to_owned(),
        });
    }
    out
}

/// The `.actors` folder of an item folder.
pub fn actors_dir(root: &Path) -> PathBuf {
    root.join(ACTORS_DIR)
}

/// Whether `dir` is a symbolic link (or a junction), so it must be left alone.
pub fn is_link(dir: &Path) -> bool {
    fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink())
}

/// The image files in `.actors` that belong to none of `wanted`. Other files are not portraits
/// and are not listed. Empty when the folder is a link or does not exist.
pub fn stale_portraits(dir: &Path, wanted: &[WantedActor]) -> Vec<PathBuf> {
    if is_link(dir) {
        return Vec::new();
    }
    let keep: HashSet<String> = wanted.iter().map(|w| w.stem.to_lowercase()).collect();
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut stale: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_image_name(p))
        .filter(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .is_none_or(|s| !keep.contains(&s.to_lowercase()))
        })
        .collect();
    stale.sort();
    stale
}

/// The portrait file of an actor in an item folder, whatever its extension.
pub fn portrait_file(root: &Path, name: &str) -> Option<PathBuf> {
    let stem = actor_file_stem(name)?;
    let dir = actors_dir(root);
    super::files::variants(&dir.join(format!("{stem}.jpg")))
        .into_iter()
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_become_file_names_with_underscores_and_without_invalid_characters() {
        assert_eq!(
            actor_file_stem("Timothée Chalamet").as_deref(),
            Some("Timothée_Chalamet")
        );
        assert_eq!(
            actor_file_stem("AC/DC: Live?").as_deref(),
            Some("ACDC_Live")
        );
        assert_eq!(
            actor_file_stem("  Samuel L. Jackson ").as_deref(),
            Some("Samuel_L._Jackson")
        );
        assert_eq!(
            actor_file_stem("a\"b\\c<d>e|f*g").as_deref(),
            Some("abcdefg")
        );
    }

    #[test]
    fn nothing_dangerous_survives_as_a_file_name() {
        assert_eq!(actor_file_stem(""), None);
        assert_eq!(actor_file_stem("???"), None);
        assert_eq!(actor_file_stem(".."), None);
        assert_eq!(actor_file_stem("../../etc"), Some("etc".to_owned()));
        assert_eq!(actor_file_stem("Con").as_deref(), Some("Con_"));
        assert_eq!(actor_file_stem("nul.x").as_deref(), Some("nul.x_"));
    }

    #[test]
    fn only_actors_are_wanted_and_clashing_names_share_the_first() {
        let mut with_photo = Credit::new(CreditKind::Actor, "Tom Hanks", "Woody");
        with_photo.person.thumb_url = " https://i.test/tom.jpg ".into();
        let cast = [
            with_photo,
            Credit::new(CreditKind::Director, "Ron Howard", ""),
            Credit::new(CreditKind::Actor, "Tom  Hanks", "Again"),
            Credit::new(CreditKind::Actor, "tom_hanks", "Lower"),
            Credit::new(CreditKind::Actor, "Meg Ryan", "Annie"),
        ];
        let wanted = wanted_actors(&cast);
        let stems: Vec<&str> = wanted.iter().map(|w| w.stem.as_str()).collect();
        assert_eq!(stems, ["Tom_Hanks", "Tom__Hanks", "Meg_Ryan"]);
        assert_eq!(wanted[0].url, "https://i.test/tom.jpg");
        assert_eq!(wanted[2].url, "");
    }

    #[test]
    fn only_images_of_actors_who_left_the_cast_are_stale() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(ACTORS_DIR);
        fs::create_dir_all(&folder).unwrap();
        for name in [
            "Meg_Ryan.jpg",
            "Tom_Hanks.PNG",
            "Old_Actor.jpg",
            "notes.txt",
        ] {
            fs::write(folder.join(name), b"x").unwrap();
        }
        let cast = [
            Credit::new(CreditKind::Actor, "Meg Ryan", ""),
            Credit::new(CreditKind::Actor, "Tom Hanks", ""),
        ];
        let stale = stale_portraits(&folder, &wanted_actors(&cast));
        let names: Vec<_> = stale
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Old_Actor.jpg"]);
        assert!(portrait_file(dir.path(), "Meg Ryan").is_some());
        assert!(portrait_file(dir.path(), "Nobody").is_none());
    }
}
