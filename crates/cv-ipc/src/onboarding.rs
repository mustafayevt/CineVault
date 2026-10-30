//! Typed commands for the first-run setup: look inside a folder to tell Movies from
//! TV shows, and open the system folder picker.
//!
//! The rules live in `cv_app::onboarding`; this module converts to DTOs and keeps the slow work
//! (walking a library folder on a possibly slow disk) off the UI thread.

use std::path::Path;

use cv_app::onboarding::{FolderAnalysis, FolderGuess, SuggestedKind, analyze_folder};
use serde::Serialize;
use specta::Type;

use crate::sources::{LibraryState, SourceCommandError};

/// A refusal: `message` is ready to show, `code` is stable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct OnboardingCommandError {
    /// `not_a_folder`, `storage`, or a library-folder rule that failed (`system_folder`,
    /// `not_absolute`, `invalid_characters`, `empty`).
    pub code: String,
    pub message: String,
}

/// What the contents of a folder look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FolderGuessDto {
    Movies,
    Tv,
    Mixed,
    Empty,
}

impl From<FolderGuess> for FolderGuessDto {
    fn from(guess: FolderGuess) -> Self {
        match guess {
            FolderGuess::Movies => Self::Movies,
            FolderGuess::Tv => Self::Tv,
            FolderGuess::Mixed => Self::Mixed,
            FolderGuess::Empty => Self::Empty,
        }
    }
}

/// The library to preselect for a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedKindDto {
    Movie,
    Tv,
    /// A mixed folder: the user has to choose.
    Undecided,
}

impl From<SuggestedKind> for SuggestedKindDto {
    fn from(kind: SuggestedKind) -> Self {
        match kind {
            SuggestedKind::Movies => Self::Movie,
            SuggestedKind::Tv => Self::Tv,
            SuggestedKind::Undecided => Self::Undecided,
        }
    }
}

/// A sub-folder of an analysed folder, for splitting a mixed one.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct FolderChildDto {
    pub path: String,
    pub name: String,
    pub guess: FolderGuessDto,
    pub suggested: SuggestedKindDto,
    pub videos: u32,
    /// The sub-folder is one TV show (its season folders or episodes sit directly in it), not a
    /// folder of shows, so its parent is the TV library folder.
    pub show: bool,
}

/// What a folder holds.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct FolderAnalysisDto {
    pub path: String,
    pub name: String,
    pub guess: FolderGuessDto,
    pub suggested: SuggestedKindDto,
    pub videos: u32,
    pub episodes: u32,
    /// The walk stopped early; the counts are lower bounds.
    pub truncated: bool,
    /// The sub-folders that hold videos, each with its own label.
    pub children: Vec<FolderChildDto>,
    /// Videos directly in the folder, outside every sub-folder. A split leaves them out.
    pub loose_videos: u32,
    /// The names of the first few of them.
    pub loose_examples: Vec<String>,
    /// The folder is itself one TV show, so its parent is the TV library folder.
    pub single_show: bool,
    /// The full path of the video the Renaming step renders its example on (the most tags).
    pub first_video: Option<String>,
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

impl From<FolderAnalysis> for FolderAnalysisDto {
    fn from(found: FolderAnalysis) -> Self {
        Self {
            path: found.path,
            name: found.name,
            guess: found.guess.into(),
            suggested: found.suggested.into(),
            videos: count(found.videos),
            episodes: count(found.episodes),
            truncated: found.truncated,
            loose_videos: count(found.loose_videos),
            loose_examples: found.loose_examples,
            single_show: found.single_show,
            first_video: found.first_video,
            children: found
                .children
                .into_iter()
                .map(|child| FolderChildDto {
                    path: child.path,
                    name: child.name,
                    guess: child.guess.into(),
                    suggested: child.suggested.into(),
                    videos: count(child.videos),
                    show: child.show,
                })
                .collect(),
        }
    }
}

/// Checks a folder against the library-folder rules, then analyses it. A folder that can never
/// be a library folder (a system folder, the app's data folder) is refused here, as soon as it
/// is chosen, with the same sentence adding it would give.
pub(crate) fn check_and_analyze(
    path: &str,
    check: impl Fn(&str) -> Result<String, SourceCommandError>,
) -> Result<FolderAnalysisDto, OnboardingCommandError> {
    check(path).map_err(|error| OnboardingCommandError {
        code: error.code,
        message: error.message,
    })?;
    analyze_folder(Path::new(path))
        .map(Into::into)
        .map_err(|error| OnboardingCommandError {
            code: "not_a_folder".to_owned(),
            message: error.to_string(),
        })
}

/// Looks inside a folder and says whether it holds movies, TV shows or both.
pub async fn analyze_library_folder(
    state: &LibraryState,
    path: String,
) -> Result<FolderAnalysisDto, OnboardingCommandError> {
    let check = state.folder_check();
    tokio::task::spawn_blocking(move || check_and_analyze(&path, check))
        .await
        .map_err(|error| OnboardingCommandError {
            code: "storage".to_owned(),
            message: error.to_string(),
        })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_analysed_into_a_dto_with_counts() {
        let dir = tempfile::tempdir().unwrap();
        let show = dir.path().join("Show/Season 1");
        std::fs::create_dir_all(&show).unwrap();
        std::fs::write(show.join("Show - S01E01.mkv"), b"x").unwrap();
        let dto = FolderAnalysisDto::from(analyze_folder(dir.path()).unwrap());
        assert_eq!(dto.guess, FolderGuessDto::Tv);
        assert_eq!(dto.suggested, SuggestedKindDto::Tv);
        assert_eq!((dto.videos, dto.episodes), (1, 1));
        assert_eq!(dto.children.len(), 1);
    }

    #[test]
    fn a_folder_that_breaks_a_library_rule_is_refused_before_it_is_analysed() {
        use cv_scan::sources::{PathStyle, SourceGuard, SystemFolders};

        let dir = tempfile::tempdir().unwrap();
        let protected = dir.path().join("AppData");
        let movies = protected.join("Temp").join("Movies");
        std::fs::create_dir_all(&movies).unwrap();
        std::fs::write(movies.join("Heat (1995).mkv"), b"x").unwrap();
        let mut system = SystemFolders::new(PathStyle::current());
        system.protect_tree(&protected.to_string_lossy(), "your application data folder");
        let guard = SourceGuard::new(PathStyle::current(), system);
        let check = |path: &str| {
            guard
                .check(path, &[], None)
                .map_err(|error| SourceCommandError {
                    code: error.code().to_owned(),
                    message: error.to_string(),
                })
        };

        let error = check_and_analyze(&movies.to_string_lossy(), check).unwrap_err();
        assert_eq!(error.code, "system_folder");
        assert!(
            error.message.contains(&format!(
                "is inside your application data folder ({})",
                protected.display()
            )),
            "{}",
            error.message
        );
        // A folder outside it is analysed as usual.
        let fine = dir.path().join("Media");
        std::fs::create_dir_all(&fine).unwrap();
        std::fs::write(fine.join("Heat (1995).mkv"), b"x").unwrap();
        let dto = check_and_analyze(&fine.to_string_lossy(), check).unwrap();
        assert_eq!(dto.videos, 1);
    }

    #[test]
    fn a_path_that_is_not_a_folder_is_refused_in_words() {
        let dir = tempfile::tempdir().unwrap();
        let error = analyze_folder(&dir.path().join("missing")).unwrap_err();
        assert!(error.to_string().contains("not a folder"));
    }
}
