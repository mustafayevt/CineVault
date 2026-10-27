//! The five naming templates of a profile, as the editor sees and changes them.

use cv_rename::RenameProfile;

use super::model::PatternField;

/// The templates of a profile: movie folder and file, show folder, season folder, episode file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternSet {
    pub movie_folder: String,
    pub movie_file: String,
    pub show_folder: String,
    pub season_folder: String,
    pub episode_file: String,
}

impl PatternSet {
    /// The templates of `profile`.
    pub fn of(profile: &RenameProfile) -> Self {
        PatternSet {
            movie_folder: profile.movie.folder_pattern.clone(),
            movie_file: profile.movie.file_pattern.clone(),
            show_folder: profile.tv.show_pattern.clone(),
            season_folder: profile.tv.season_pattern.clone(),
            episode_file: profile.tv.file_pattern.clone(),
        }
    }

    /// Writes the templates into `profile`; its other options stay.
    pub fn apply_to(&self, profile: &mut RenameProfile) {
        profile.movie.folder_pattern.clone_from(&self.movie_folder);
        profile.movie.file_pattern.clone_from(&self.movie_file);
        profile.tv.show_pattern.clone_from(&self.show_folder);
        profile.tv.season_pattern.clone_from(&self.season_folder);
        profile.tv.file_pattern.clone_from(&self.episode_file);
    }

    /// One template by field.
    pub fn get(&self, field: PatternField) -> &str {
        match field {
            PatternField::MovieFolder => &self.movie_folder,
            PatternField::MovieFile => &self.movie_file,
            PatternField::ShowFolder => &self.show_folder,
            PatternField::SeasonFolder => &self.season_folder,
            PatternField::EpisodeFile => &self.episode_file,
        }
    }
}
