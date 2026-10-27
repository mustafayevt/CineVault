//! Naming profiles: the three presets and the user's custom profiles.
//!
//! A [`RenameProfile`] is a name with the [`MovieSettings`] and [`TvSettings`] the planners read.
//! The presets are built in and cannot be changed; a custom profile starts as a copy of any
//! profile and can be edited, renamed and deleted. A [`ProfileBook`] holds the custom profiles
//! and says which profile is active; Organize, the executor and rename-after-scrape all use the
//! active one. The book is plain data: the settings layer stores it (`cv-app`).

use crate::options::{MovieSettings, TvSettings};
use crate::preset::Preset;

/// The id of the profile that is active until the user picks another.
pub const DEFAULT_PROFILE_ID: &str = "clean";

/// The longest profile name, in characters.
pub const MAX_NAME_CHARS: usize = 60;

/// One naming profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameProfile {
    /// Stable id: the preset's id, or `custom-<name>` for a custom profile.
    pub id: String,
    pub name: String,
    /// `Some` for a built-in preset, which cannot be edited or deleted.
    pub preset: Option<Preset>,
    pub movie: MovieSettings,
    pub tv: TvSettings,
}

impl RenameProfile {
    /// The built-in profile of a preset.
    pub fn from_preset(preset: Preset) -> Self {
        RenameProfile {
            id: preset.id().to_owned(),
            name: preset.label().to_owned(),
            preset: Some(preset),
            movie: preset.movie_settings(),
            tv: preset.tv_settings(),
        }
    }

    pub fn is_builtin(&self) -> bool {
        self.preset.is_some()
    }
}

/// Why a change to the profiles was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    #[error("there is no profile `{0}`")]
    NotFound(String),
    #[error("the preset `{0}` is built in and cannot be changed or deleted")]
    BuiltIn(String),
    #[error("a profile needs a name")]
    EmptyName,
    #[error("a profile name can have at most {MAX_NAME_CHARS} characters")]
    NameTooLong,
    #[error("a profile called `{0}` exists already")]
    DuplicateName(String),
}

/// The custom profiles and the active profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileBook {
    custom: Vec<RenameProfile>,
    active: String,
}

impl Default for ProfileBook {
    fn default() -> Self {
        ProfileBook {
            custom: Vec::new(),
            active: DEFAULT_PROFILE_ID.to_owned(),
        }
    }
}

impl ProfileBook {
    /// A book of stored custom profiles. A profile that repeats an id or a name already taken, or
    /// carries a preset's id, is dropped; an `active` id that does not exist becomes the default.
    pub fn new(custom: Vec<RenameProfile>, active: &str) -> Self {
        let mut book = ProfileBook::default();
        for mut profile in custom {
            profile.preset = None;
            let name = profile.name.trim().to_owned();
            if name.is_empty()
                || Preset::parse(&profile.id).is_some()
                || book.get(&profile.id).is_some()
                || book.name_taken(&name, None)
            {
                continue;
            }
            profile.name = name;
            book.custom.push(profile);
        }
        if book.get(active).is_some() {
            book.active = active.to_owned();
        }
        book
    }

    /// The custom profiles, in the order they were made.
    pub fn custom(&self) -> &[RenameProfile] {
        &self.custom
    }

    /// The profile with this id (a preset's id or a custom id), as a copy.
    pub fn get(&self, id: &str) -> Option<RenameProfile> {
        if let Some(preset) = Preset::parse(id) {
            return Some(RenameProfile::from_preset(preset));
        }
        self.custom.iter().find(|p| p.id == id).cloned()
    }

    /// The ids and names of all profiles for a picker: presets first.
    pub fn list(&self) -> Vec<(String, String, bool)> {
        Preset::ALL
            .iter()
            .map(|p| (p.id().to_owned(), p.label().to_owned(), true))
            .chain(
                self.custom
                    .iter()
                    .map(|p| (p.id.clone(), p.name.clone(), false)),
            )
            .collect()
    }

    /// The id of the active profile.
    pub fn active_id(&self) -> &str {
        &self.active
    }

    /// The active profile. It always exists: deleting the active profile makes the default
    /// active.
    pub fn active(&self) -> RenameProfile {
        self.get(&self.active)
            .unwrap_or_else(|| RenameProfile::from_preset(Preset::Clean))
    }

    /// Makes a profile the active one.
    ///
    /// # Errors
    /// [`ProfileError::NotFound`] when there is no such profile.
    pub fn set_active(&mut self, id: &str) -> Result<(), ProfileError> {
        let found = self
            .get(id)
            .ok_or_else(|| ProfileError::NotFound(id.to_owned()))?;
        self.active = found.id;
        Ok(())
    }

    /// Creates a custom profile as a copy of profile `from` (a preset or a custom profile) and
    /// returns it. This is also how a profile is copied.
    ///
    /// # Errors
    /// [`ProfileError::NotFound`] for an unknown `from`; a name error when `name` is empty, too
    /// long or used by another profile (the presets' names count).
    pub fn create(&mut self, name: &str, from: &str) -> Result<RenameProfile, ProfileError> {
        let source = self
            .get(from)
            .ok_or_else(|| ProfileError::NotFound(from.to_owned()))?;
        let name = self.check_name(name, None)?;
        let profile = RenameProfile {
            id: self.fresh_id(&name),
            name,
            preset: None,
            movie: source.movie,
            tv: source.tv,
        };
        self.custom.push(profile.clone());
        Ok(profile)
    }

    /// Renames a custom profile.
    ///
    /// # Errors
    /// [`ProfileError::BuiltIn`] for a preset, plus the errors of [`ProfileBook::create`].
    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), ProfileError> {
        self.require_custom(id)?;
        let name = self.check_name(name, Some(id))?;
        if let Some(profile) = self.custom.iter_mut().find(|p| p.id == id) {
            profile.name = name;
        }
        Ok(())
    }

    /// Replaces the patterns and options of a custom profile.
    ///
    /// # Errors
    /// [`ProfileError::BuiltIn`] for a preset, [`ProfileError::NotFound`] for an unknown id.
    pub fn update(
        &mut self,
        id: &str,
        movie: MovieSettings,
        tv: TvSettings,
    ) -> Result<(), ProfileError> {
        self.require_custom(id)?;
        if let Some(profile) = self.custom.iter_mut().find(|p| p.id == id) {
            profile.movie = movie;
            profile.tv = tv;
        }
        Ok(())
    }

    /// Deletes a custom profile. When it was the active one the default becomes active.
    ///
    /// # Errors
    /// [`ProfileError::BuiltIn`] for a preset, [`ProfileError::NotFound`] for an unknown id.
    pub fn delete(&mut self, id: &str) -> Result<(), ProfileError> {
        self.require_custom(id)?;
        self.custom.retain(|p| p.id != id);
        if self.active == id {
            DEFAULT_PROFILE_ID.clone_into(&mut self.active);
        }
        Ok(())
    }

    fn require_custom(&self, id: &str) -> Result<(), ProfileError> {
        if Preset::parse(id).is_some() {
            return Err(ProfileError::BuiltIn(id.to_owned()));
        }
        if self.custom.iter().any(|p| p.id == id) {
            Ok(())
        } else {
            Err(ProfileError::NotFound(id.to_owned()))
        }
    }

    fn check_name(&self, name: &str, own_id: Option<&str>) -> Result<String, ProfileError> {
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            return Err(ProfileError::EmptyName);
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(ProfileError::NameTooLong);
        }
        if self.name_taken(&name, own_id) {
            return Err(ProfileError::DuplicateName(name));
        }
        Ok(name)
    }

    /// True when a preset or another custom profile has this name, ignoring case.
    fn name_taken(&self, name: &str, own_id: Option<&str>) -> bool {
        Preset::ALL
            .iter()
            .any(|p| p.label().eq_ignore_ascii_case(name))
            || self
                .custom
                .iter()
                .any(|p| Some(p.id.as_str()) != own_id && p.name.eq_ignore_ascii_case(name))
    }

    /// `custom-<name as a slug>`, numbered when taken.
    fn fresh_id(&self, name: &str) -> String {
        let mut slug = String::new();
        for c in name.chars() {
            if c.is_alphanumeric() {
                slug.extend(c.to_lowercase());
            } else if !slug.ends_with('-') {
                slug.push('-');
            }
        }
        let slug = slug.trim_matches('-');
        let base = if slug.is_empty() {
            "custom".to_owned()
        } else {
            format!("custom-{slug}")
        };
        let mut id = base.clone();
        let mut n = 2;
        while self.custom.iter().any(|p| p.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_book_has_the_clean_preset_active() {
        let book = ProfileBook::default();
        assert_eq!(book.active_id(), "clean");
        let active = book.active();
        assert_eq!(active.preset, Some(Preset::Clean));
        assert_eq!(active.movie, Preset::Clean.movie_settings());
        assert_eq!(book.list().len(), 3);
    }

    #[test]
    fn create_copies_a_preset_and_edits_stay_in_the_copy() {
        let mut book = ProfileBook::default();
        let made = book.create("My  Library", "detailed").unwrap();
        assert_eq!(made.id, "custom-my-library");
        assert_eq!(made.name, "My Library");
        assert!(!made.is_builtin());
        assert_eq!(made.movie, Preset::Detailed.movie_settings());

        let mut movie = made.movie.clone();
        movie.folder_pattern = "${title}".to_owned();
        book.update(&made.id, movie, made.tv.clone()).unwrap();
        assert_eq!(book.get(&made.id).unwrap().movie.folder_pattern, "${title}");
        assert_eq!(
            book.get("detailed").unwrap().movie,
            Preset::Detailed.movie_settings()
        );
    }

    #[test]
    fn a_custom_profile_can_be_copied_renamed_and_deleted() {
        let mut book = ProfileBook::default();
        let first = book.create("Mine", "clean").unwrap();
        let copy = book.create("Mine copy", &first.id).unwrap();
        assert_eq!(copy.movie, first.movie);
        book.rename(&copy.id, "Archive").unwrap();
        assert_eq!(book.get(&copy.id).unwrap().name, "Archive");
        book.set_active(&copy.id).unwrap();
        assert_eq!(book.active_id(), copy.id);
        book.delete(&copy.id).unwrap();
        assert!(book.get(&copy.id).is_none());
        assert_eq!(book.active_id(), "clean");
        assert_eq!(book.custom().len(), 1);
    }

    #[test]
    fn presets_are_read_only() {
        let mut book = ProfileBook::default();
        let movie = Preset::Clean.movie_settings();
        let tv = Preset::Clean.tv_settings();
        assert_eq!(
            book.update("clean", movie, tv),
            Err(ProfileError::BuiltIn("clean".into()))
        );
        assert_eq!(
            book.delete("detailed"),
            Err(ProfileError::BuiltIn("detailed".into()))
        );
        assert_eq!(
            book.rename("media-server-safe", "x"),
            Err(ProfileError::BuiltIn("media-server-safe".into()))
        );
        assert_eq!(
            book.delete("nope"),
            Err(ProfileError::NotFound("nope".into()))
        );
    }

    #[test]
    fn names_are_checked() {
        let mut book = ProfileBook::default();
        assert_eq!(book.create("  ", "clean"), Err(ProfileError::EmptyName));
        assert_eq!(
            book.create(&"x".repeat(61), "clean"),
            Err(ProfileError::NameTooLong)
        );
        assert_eq!(
            book.create("detailed", "clean"),
            Err(ProfileError::DuplicateName("detailed".into()))
        );
        book.create("Mine", "clean").unwrap();
        assert_eq!(
            book.create("MINE", "clean"),
            Err(ProfileError::DuplicateName("MINE".into()))
        );
        assert_eq!(
            book.create("Other", "missing"),
            Err(ProfileError::NotFound("missing".into()))
        );
    }

    #[test]
    fn ids_stay_unique_when_names_slug_alike() {
        let mut book = ProfileBook::default();
        let a = book.create("A b", "clean").unwrap();
        let b = book.create("A-b", "clean").unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(b.id, "custom-a-b-2");
        let symbols = book.create("***", "clean").unwrap();
        assert_eq!(symbols.id, "custom");
    }

    #[test]
    fn a_stored_book_drops_what_cannot_be_kept() {
        let mut book = ProfileBook::default();
        let a = book.create("A", "clean").unwrap();
        let mut dup_id = a.clone();
        dup_id.name = "Other".into();
        let mut preset_id = a.clone();
        preset_id.id = "clean".into();
        preset_id.name = "Third".into();
        let mut unnamed = a.clone();
        unnamed.id = "custom-x".into();
        unnamed.name = " ".into();
        let loaded = ProfileBook::new(vec![a.clone(), dup_id, preset_id, unnamed], "gone");
        assert_eq!(loaded.custom(), std::slice::from_ref(&a));
        assert_eq!(loaded.active_id(), "clean");
        let kept = ProfileBook::new(vec![a.clone()], &a.id);
        assert_eq!(kept.active_id(), a.id);
    }
}
