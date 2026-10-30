//! Typed commands for Organize: profiles and templates, the preview, apply, the journal and undo.
//!
//! The rules live in `cv_app::organize` and `cv-rename`; this module opens the service on first
//! use (it shares the library database with the other commands), reads the naming profiles and
//! the NFO file names from the settings, runs the work off the UI thread and converts the
//! answers into DTOs.
//!
//! Preview and apply take the same request (scope, profile id, the editor's templates and the
//! user's conflict choices), so Apply plans again and judges the disk as it is when the button is
//! pressed. Nothing here touches the disk before [`organize_apply`] or [`organize_undo`].
//!
//! [`recover_at_startup`] is the start-up hook: the composition root calls it once, before the
//! first scan, to settle what a crash left half done.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use cv_app::nfo::NfoSettings;
use cv_app::organize::{
    self, Applied, AppliedStatus, Choice, ConflictCode, ConflictRow, Counts, Example, ExampleKind,
    FindingSeverity, JournalBatch, OrganizeError, OrganizeService, PatternField, PatternSet,
    Preview, PreviewRequest, PreviewRow, ResolutionOption, RowKind, RowStatus, Scope, UndoOutcome,
};
use cv_app::rename_settings::{self, RenameSettings};
use cv_artwork::library_source::LibrarySources;
use cv_artwork::naming::ArtworkNames;
use cv_db::repo::rename_journal::PathObserver;
use cv_domain::{ItemId, MediaFileType};
use cv_rename::{ProfileError, RenameProfile};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::settings::SettingsState;
use crate::sources::LibraryState;

/// Held in Tauri state. The service is built by the first command that needs it.
pub struct OrganizeState {
    service: OnceLock<Result<Arc<OrganizeService>, String>>,
    /// The poster index behind `cv://img`; told about every move so thumbnails follow renames.
    sources: Arc<LibrarySources>,
}

impl OrganizeState {
    pub fn new(sources: Arc<LibrarySources>) -> Self {
        Self {
            service: OnceLock::new(),
            sources,
        }
    }

    fn service(
        &self,
        library: &LibraryState,
    ) -> Result<Arc<OrganizeService>, OrganizeCommandError> {
        self.service
            .get_or_init(|| {
                let db = library.database()?;
                let observer: Arc<dyn PathObserver> = self.sources.clone();
                // An undo of a move between drives restores originals from the Recycle Bin.
                let mut service = OrganizeService::new(db)
                    .with_path_observer(observer)
                    .with_bin(library.bin())
                    .with_choice(library.deleted_files());
                if let Some(state) = library.organize_state() {
                    service = service.with_changed_hook(state.hook());
                }
                Ok(Arc::new(service))
            })
            .as_ref()
            .map(Arc::clone)
            .map_err(|message| OrganizeCommandError {
                code: "storage".to_owned(),
                message: message.clone(),
            })
    }
}

// ---- errors --------------------------------------------------------------------------------

/// Why a command did not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct OrganizeCommandError {
    /// `storage`, `settings`, `profile`, `invalid_id`, `unknown_batch`, `template`, `unsettled`, `move` or
    /// `journal`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<OrganizeError> for OrganizeCommandError {
    fn from(error: OrganizeError) -> Self {
        let code = match &error {
            OrganizeError::Db(_) => "storage",
            OrganizeError::Rename(_) => "template",
            OrganizeError::Exec(_) | OrganizeError::Journal(_) => "journal",
            OrganizeError::Undo(_) => "unsettled",
            OrganizeError::UnknownBatch(_) => "unknown_batch",
            OrganizeError::Move(_) => "move",
        };
        Self {
            code: code.to_owned(),
            message: error.to_string(),
        }
    }
}

impl From<ProfileError> for OrganizeCommandError {
    fn from(error: ProfileError) -> Self {
        Self {
            code: "profile".to_owned(),
            message: error.to_string(),
        }
    }
}

fn settings_error(error: impl std::fmt::Display) -> OrganizeCommandError {
    OrganizeCommandError {
        code: "settings".to_owned(),
        message: format!("The naming profile could not be saved: {error}"),
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, OrganizeCommandError> + Send + 'static,
) -> Result<T, OrganizeCommandError> {
    cv_jobs::client::spawn_blocking(work)
        .await
        .map_err(|error| OrganizeCommandError {
            code: "storage".to_owned(),
            message: format!("Organize stopped unexpectedly: {error}"),
        })?
}

// ---- profiles ------------------------------------------------------------------------------

/// The five naming templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PatternsDto {
    pub movie_folder: String,
    pub movie_file: String,
    pub show_folder: String,
    pub season_folder: String,
    pub episode_file: String,
}

impl From<PatternSet> for PatternsDto {
    fn from(set: PatternSet) -> Self {
        Self {
            movie_folder: set.movie_folder,
            movie_file: set.movie_file,
            show_folder: set.show_folder,
            season_folder: set.season_folder,
            episode_file: set.episode_file,
        }
    }
}

impl From<PatternsDto> for PatternSet {
    fn from(dto: PatternsDto) -> Self {
        Self {
            movie_folder: dto.movie_folder,
            movie_file: dto.movie_file,
            show_folder: dto.show_folder,
            season_folder: dto.season_folder,
            episode_file: dto.episode_file,
        }
    }
}

/// A naming profile: a preset or a custom one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct OrganizeProfileDto {
    /// `clean`, `detailed`, `media-server-safe` or `custom-...`.
    pub id: String,
    pub name: String,
    /// A preset, which cannot be changed or deleted.
    pub builtin: bool,
    pub patterns: PatternsDto,
}

/// The profiles and which one is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct OrganizeProfilesDto {
    pub active: String,
    pub profiles: Vec<OrganizeProfileDto>,
}

fn profile_dto(profile: &RenameProfile) -> OrganizeProfileDto {
    OrganizeProfileDto {
        id: profile.id.clone(),
        name: profile.name.clone(),
        builtin: profile.is_builtin(),
        patterns: PatternSet::of(profile).into(),
    }
}

fn profiles_dto(settings: &RenameSettings) -> OrganizeProfilesDto {
    let profiles = settings
        .book
        .list()
        .into_iter()
        .filter_map(|(id, _, _)| settings.book.get(&id))
        .map(|p| profile_dto(&p))
        .collect();
    OrganizeProfilesDto {
        active: settings.book.active_id().to_owned(),
        profiles,
    }
}

fn rename_settings(state: &SettingsState) -> RenameSettings {
    RenameSettings::from_values(&state.service().snapshot().values)
}

fn nfo_settings(state: &SettingsState) -> NfoSettings {
    NfoSettings::from_values(&state.service().snapshot().values)
}

fn artwork_names(state: &SettingsState) -> ArtworkNames {
    ArtworkNames::from_values(&state.service().snapshot().values)
}

fn store(state: &SettingsState, settings: &RenameSettings) -> Result<(), OrganizeCommandError> {
    for (key, value) in settings.to_values() {
        state.service().set(key, value).map_err(settings_error)?;
    }
    Ok(())
}

/// The naming profiles (the presets and the user's own) and the active one.
pub fn organize_profiles(settings: &SettingsState) -> OrganizeProfilesDto {
    profiles_dto(&rename_settings(settings))
}

/// Makes a profile the active one (the one rename-after-scrape uses too).
pub fn organize_select_profile(
    settings: &SettingsState,
    id: String,
) -> Result<OrganizeProfilesDto, OrganizeCommandError> {
    let mut current = rename_settings(settings);
    current.book.set_active(&id)?;
    store(settings, &current)?;
    Ok(profiles_dto(&current))
}

/// Saves the editor's templates as a new custom profile named `name`, copied from `from` for
/// everything but the templates, and makes it the active one.
pub fn organize_save_profile(
    settings: &SettingsState,
    name: String,
    from: String,
    patterns: PatternsDto,
) -> Result<OrganizeProfilesDto, OrganizeCommandError> {
    let mut current = rename_settings(settings);
    let created = current.book.create(&name, &from)?;
    let mut profile = created.clone();
    PatternSet::from(patterns).apply_to(&mut profile);
    current
        .book
        .update(&created.id, profile.movie, profile.tv)?;
    current.book.set_active(&created.id)?;
    store(settings, &current)?;
    Ok(profiles_dto(&current))
}

/// Replaces the templates of a custom profile.
pub fn organize_update_profile(
    settings: &SettingsState,
    id: String,
    patterns: PatternsDto,
) -> Result<OrganizeProfilesDto, OrganizeCommandError> {
    let mut current = rename_settings(settings);
    let mut profile = current
        .book
        .get(&id)
        .ok_or_else(|| ProfileError::NotFound(id.clone()))?;
    PatternSet::from(patterns).apply_to(&mut profile);
    current.book.update(&id, profile.movie, profile.tv)?;
    store(settings, &current)?;
    Ok(profiles_dto(&current))
}

/// Deletes a custom profile. When it was the active one the default becomes active.
pub fn organize_delete_profile(
    settings: &SettingsState,
    id: String,
) -> Result<OrganizeProfilesDto, OrganizeCommandError> {
    let mut current = rename_settings(settings);
    current.book.delete(&id)?;
    store(settings, &current)?;
    Ok(profiles_dto(&current))
}

// ---- template help -------------------------------------------------------------------------

/// Which template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PatternFieldDto {
    MovieFolder,
    MovieFile,
    ShowFolder,
    SeasonFolder,
    EpisodeFile,
}

impl From<PatternFieldDto> for PatternField {
    fn from(field: PatternFieldDto) -> Self {
        match field {
            PatternFieldDto::MovieFolder => Self::MovieFolder,
            PatternFieldDto::MovieFile => Self::MovieFile,
            PatternFieldDto::ShowFolder => Self::ShowFolder,
            PatternFieldDto::SeasonFolder => Self::SeasonFolder,
            PatternFieldDto::EpisodeFile => Self::EpisodeFile,
        }
    }
}

impl From<PatternField> for PatternFieldDto {
    fn from(field: PatternField) -> Self {
        match field {
            PatternField::MovieFolder => Self::MovieFolder,
            PatternField::MovieFile => Self::MovieFile,
            PatternField::ShowFolder => Self::ShowFolder,
            PatternField::SeasonFolder => Self::SeasonFolder,
            PatternField::EpisodeFile => Self::EpisodeFile,
        }
    }
}

/// One finding about a template as typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TemplateFindingDto {
    /// `error` or `warning`.
    pub severity: String,
    pub message: String,
    /// Where the marked text starts and how long it is, in UTF-16 code units of the template.
    pub offset: u32,
    pub len: u32,
    pub suggestions: Vec<String>,
}

/// Checks a template as typed: unknown tokens with suggestions, syntax errors with positions and
/// the TV rules for the episode file template.
pub fn organize_check_template(
    field: PatternFieldDto,
    template: String,
) -> Vec<TemplateFindingDto> {
    organize::check_pattern(field.into(), &template)
        .into_iter()
        .map(|f| TemplateFindingDto {
            severity: match f.severity {
                FindingSeverity::Error => "error",
                FindingSeverity::Warning => "warning",
            }
            .to_owned(),
            message: f.message,
            offset: f.offset,
            len: f.len,
            suggestions: f.suggestions,
        })
        .collect()
}

/// A movie file name rendered under one profile's templates, for the first run's Renaming step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FileExampleDto {
    /// The file name the example starts from.
    pub current: String,
    /// The folder the profile gives.
    pub folder: String,
    /// The file name the profile gives, with its extension.
    pub file: String,
    pub title: String,
    pub year: u32,
    /// `current` is the built-in sample, not a file of the user.
    pub sample: bool,
}

/// Renders a profile's movie templates on one file name, or on a built-in sample when no name is
/// given. Nothing is read from disk and nothing is saved: the first run uses it before any
/// library exists.
pub fn organize_example_for_file(
    settings: &SettingsState,
    profile_id: String,
    file_name: Option<String>,
) -> Result<FileExampleDto, OrganizeCommandError> {
    let current = rename_settings(settings);
    let profile = current
        .book
        .get(&profile_id)
        .ok_or_else(|| ProfileError::NotFound(profile_id.clone()))?;
    let example = organize::example_for_file(&profile, file_name.as_deref()).ok_or_else(|| {
        OrganizeCommandError {
            code: "template".to_owned(),
            message: "The templates of this profile could not be shown on an example.".to_owned(),
        }
    })?;
    Ok(FileExampleDto {
        current: example.current,
        folder: example.folder,
        file: example.file,
        title: example.title,
        year: u32::from(example.year),
        sample: example.sample,
    })
}

/// A token a template can use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TemplateTokenDto {
    /// The name written in `${...}`.
    pub name: String,
    /// The model path it stands for, with any renderer.
    pub target: String,
}

/// The tokens of the movie templates (`tv` false) or the TV templates, for the `$` autocomplete.
pub fn organize_tokens(tv: bool) -> Vec<TemplateTokenDto> {
    organize::token_catalogue(tv)
        .into_iter()
        .map(|t| TemplateTokenDto {
            name: t.name,
            target: t.target,
        })
        .collect()
}

// ---- preview and apply ---------------------------------------------------------------------

/// What Organize looks at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OrganizeScopeDto {
    /// The selected library items (ids as the library views hold them).
    Selection { ids: Vec<String> },
    /// Every movie and show.
    Everything,
}

/// What the user chose for a conflicting item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ChoiceKindDto {
    KeepAsVersions,
    Skip,
    Edit,
}

/// A conflict choice for one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ChoiceDto {
    pub id: String,
    pub kind: ChoiceKindDto,
    /// For `edit`: the new destination folder.
    pub folder: Option<String>,
    /// For `edit`: the new video base name.
    pub base: Option<String>,
}

/// A preview or apply request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OrganizeRequestDto {
    pub scope: OrganizeScopeDto,
    /// The profile the templates started from; its other options (clean-up, spaces) are used.
    pub profile_id: String,
    /// The templates as the editor holds them now.
    pub patterns: PatternsDto,
    pub choices: Vec<ChoiceDto>,
}

fn parse_id(text: &str) -> Result<ItemId, OrganizeCommandError> {
    text.trim()
        .parse::<i64>()
        .map(ItemId)
        .map_err(|_| OrganizeCommandError {
            code: "invalid_id".to_owned(),
            message: format!("{text:?} is not a library item id"),
        })
}

fn to_request(
    settings: &SettingsState,
    dto: OrganizeRequestDto,
) -> Result<PreviewRequest, OrganizeCommandError> {
    let scope = match dto.scope {
        OrganizeScopeDto::Everything => Scope::Everything,
        OrganizeScopeDto::Selection { ids } => Scope::Items(
            ids.iter()
                .map(|id| parse_id(id))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    };
    let current = rename_settings(settings);
    let mut profile = current
        .book
        .get(&dto.profile_id)
        .unwrap_or_else(|| current.book.active());
    rename_settings::apply_nfo_choices(&mut profile, &nfo_settings(settings));
    rename_settings::apply_artwork_choices(&mut profile, &artwork_names(settings));
    let values = settings.service().snapshot().values;
    rename_settings::apply_cleanup_choices(
        &mut profile,
        &rename_settings::CleanupChoices::from_values(&values),
    );
    rename_settings::apply_subfolder_choice(
        &mut profile,
        rename_settings::keep_subfolders(&values),
    );
    rename_settings::apply_colon_choice(&mut profile, &values);
    PatternSet::from(dto.patterns).apply_to(&mut profile);
    let mut choices = BTreeMap::new();
    for choice in dto.choices {
        choices.insert(
            choice.id,
            match choice.kind {
                ChoiceKindDto::KeepAsVersions => Choice::KeepAsVersions,
                ChoiceKindDto::Skip => Choice::Skip,
                ChoiceKindDto::Edit => Choice::Edit {
                    folder: choice.folder,
                    base: choice.base,
                },
            },
        );
    }
    Ok(PreviewRequest {
        scope,
        profile,
        choices,
    })
}

/// `movie` or `show`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RowKindDto {
    Movie,
    Show,
}

/// What a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RowStatusDto {
    /// A clean change that can be applied.
    Change,
    /// Blocked until the user resolves the conflict.
    Conflict,
    /// The user chose to skip it.
    Skipped,
    /// It cannot be planned; `note` says why.
    Unplanned,
    /// It is not scraped yet, so it keeps its names until it is.
    Unscraped,
    /// Held back: the names use media-info tokens and the video details are not read yet;
    /// `note` says why. Reading the details lets it be renamed.
    MediaUnread,
}

/// What moves along with an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct CompanionsDto {
    pub nfo: u32,
    pub artwork: u32,
    pub subtitles: u32,
    pub trailers: u32,
    pub other: u32,
}

/// One file an item moves, copies or sends to the trash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FileChangeDto {
    /// `video`, `nfo`, `artwork`, `subtitle`, `trailer` or `other`.
    pub kind: String,
    /// Where the file is now, relative to the data source root, with `/`.
    pub from: String,
    /// Where it goes: moved to the first, copied to the others. Empty when it goes to the trash
    /// folder.
    pub to: Vec<String>,
}

fn file_kind(kind: MediaFileType) -> &'static str {
    match kind {
        MediaFileType::Video => "video",
        MediaFileType::Nfo => "nfo",
        MediaFileType::Subtitle => "subtitle",
        MediaFileType::Trailer => "trailer",
        kind if kind.is_graphic() => "artwork",
        _ => "other",
    }
}

/// One item of the diff list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PreviewRowDto {
    pub id: String,
    pub kind: RowKindDto,
    pub title: String,
    pub year: u32,
    /// The data source root with a trailing separator.
    pub root: String,
    pub before: String,
    pub after: String,
    pub before_file: String,
    pub after_file: String,
    pub status: RowStatusDto,
    /// The reason, for `unplanned`.
    pub note: String,
    pub moves: u32,
    pub trashed: u32,
    pub companions: CompanionsDto,
    /// Every file that moves, is copied or goes to the trash: the video, its companions, a
    /// show's episodes.
    pub files: Vec<FileChangeDto>,
}

impl From<PreviewRow> for PreviewRowDto {
    fn from(row: PreviewRow) -> Self {
        let (status, note) = match row.status {
            RowStatus::Change => (RowStatusDto::Change, String::new()),
            RowStatus::Conflict => (RowStatusDto::Conflict, String::new()),
            RowStatus::Skipped => (RowStatusDto::Skipped, String::new()),
            RowStatus::Unplanned(why) => (RowStatusDto::Unplanned, why),
            RowStatus::Unscraped => (RowStatusDto::Unscraped, String::new()),
            RowStatus::MediaUnread => (
                RowStatusDto::MediaUnread,
                cv_rename::MEDIA_HELD_REASON.to_owned(),
            ),
        };
        Self {
            id: row.id,
            kind: match row.kind {
                RowKind::Movie => RowKindDto::Movie,
                RowKind::Show => RowKindDto::Show,
            },
            title: row.title,
            year: u32::from(row.year),
            root: row.root,
            before: row.before,
            after: row.after,
            before_file: row.before_file,
            after_file: row.after_file,
            status,
            note,
            moves: row.moves,
            trashed: row.trashed,
            companions: CompanionsDto {
                nfo: row.companions.nfo,
                artwork: row.companions.artwork,
                subtitles: row.companions.subtitles,
                trailers: row.companions.trailers,
                other: row.companions.other,
            },
            files: row
                .files
                .into_iter()
                .map(|f| FileChangeDto {
                    kind: file_kind(f.kind).to_owned(),
                    from: f.from,
                    to: f.to,
                })
                .collect(),
        }
    }
}

/// What a conflict is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConflictCodeDto {
    Duplicate,
    Foreign,
    PathTooLong,
    NameTooLong,
    CaseOnly,
}

/// A resolution on offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionOptionDto {
    KeepAsVersions,
    Skip,
    Edit,
}

/// A conflict between items, or between an item and what is on the disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ConflictDto {
    pub code: ConflictCodeDto,
    /// For `foreign`: the title that owns the destination, when the library knows it.
    pub owner: Option<String>,
    /// For the length conflicts: the length found and the limit.
    pub length: u32,
    pub limit: u32,
    pub items: Vec<String>,
    pub titles: Vec<String>,
    pub folders: Vec<String>,
    /// The destination, relative to the data source root.
    pub path: String,
    pub options: Vec<ResolutionOptionDto>,
    /// For each of `items`, the first free numbered folder name, for "Use Crash (2004) (2)".
    /// Empty for an item the preview has no row for.
    pub suggestions: Vec<String>,
}

impl From<ConflictRow> for ConflictDto {
    fn from(row: ConflictRow) -> Self {
        let (code, owner, length, limit) = match row.code {
            ConflictCode::Duplicate => (ConflictCodeDto::Duplicate, None, 0, 0),
            ConflictCode::Foreign { owner } => (ConflictCodeDto::Foreign, owner, 0, 0),
            ConflictCode::PathTooLong { length, limit } => {
                (ConflictCodeDto::PathTooLong, None, length, limit)
            }
            ConflictCode::NameTooLong { length, limit } => {
                (ConflictCodeDto::NameTooLong, None, length, limit)
            }
            ConflictCode::CaseOnly => (ConflictCodeDto::CaseOnly, None, 0, 0),
        };
        Self {
            code,
            owner,
            length,
            limit,
            items: row.items,
            titles: row.titles,
            folders: row.folders,
            path: row.path,
            suggestions: Vec::new(),
            options: row
                .options
                .into_iter()
                .map(|o| match o {
                    ResolutionOption::KeepAsVersions => ResolutionOptionDto::KeepAsVersions,
                    ResolutionOption::Skip => ResolutionOptionDto::Skip,
                    ResolutionOption::Edit => ResolutionOptionDto::Edit,
                })
                .collect(),
        }
    }
}

/// The numbers above the diff list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct CountsDto {
    pub change: u32,
    pub tidy: u32,
    pub conflicts: u32,
    pub locked: u32,
    pub skipped: u32,
    pub unplanned: u32,
    pub moves: u32,
}

impl From<Counts> for CountsDto {
    fn from(c: Counts) -> Self {
        Self {
            change: c.change,
            tidy: c.tidy,
            conflicts: c.conflicts,
            locked: c.locked,
            skipped: c.skipped,
            unplanned: c.unplanned,
            moves: c.moves,
        }
    }
}

/// One live example of the templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ExampleDto {
    /// `edition`, `no_hdr`, `colon`, `plain` or `episode`.
    pub kind: String,
    pub title: String,
    pub year: u32,
    pub folder: String,
    pub file: String,
    /// A built-in sample, not a title of the library.
    pub sample: bool,
}

impl From<Example> for ExampleDto {
    fn from(e: Example) -> Self {
        Self {
            kind: match e.kind {
                ExampleKind::Edition => "edition",
                ExampleKind::NoHdr => "no_hdr",
                ExampleKind::Colon => "colon",
                ExampleKind::Plain => "plain",
                ExampleKind::Episode => "episode",
            }
            .to_owned(),
            title: e.title,
            year: u32::from(e.year),
            folder: e.folder,
            file: e.file,
            sample: e.sample,
        }
    }
}

/// A template that does not work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TemplateProblemDto {
    pub field: PatternFieldDto,
    pub message: String,
}

/// The preview of an Organize request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct OrganizePreviewDto {
    pub rows: Vec<PreviewRowDto>,
    pub conflicts: Vec<ConflictDto>,
    pub counts: CountsDto,
    pub examples: Vec<ExampleDto>,
    /// Set when a template is broken; then `rows` is empty.
    pub problems: Vec<TemplateProblemDto>,
}

impl From<Preview> for OrganizePreviewDto {
    fn from(p: Preview) -> Self {
        let suggestions: Vec<Vec<String>> = p
            .conflicts
            .iter()
            .map(|conflict| organize::numbered_names(&p, conflict))
            .collect();
        Self {
            rows: p.rows.into_iter().map(Into::into).collect(),
            conflicts: p
                .conflicts
                .into_iter()
                .zip(suggestions)
                .map(|(conflict, suggestions)| ConflictDto {
                    suggestions,
                    ..conflict.into()
                })
                .collect(),
            counts: p.counts.into(),
            examples: p.examples.into_iter().map(Into::into).collect(),
            problems: p
                .problems
                .into_iter()
                .map(|p| TemplateProblemDto {
                    field: p.field.into(),
                    message: p.message,
                })
                .collect(),
        }
    }
}

/// Plans the request and describes it: the diff rows, conflicts, counts and live examples.
/// Nothing touches the disk.
pub async fn organize_preview(
    library: &LibraryState,
    organize: &OrganizeState,
    settings: &SettingsState,
    request: OrganizeRequestDto,
) -> Result<OrganizePreviewDto, OrganizeCommandError> {
    let service = organize.service(library)?;
    let request = to_request(settings, request)?;
    // The preview of the saved style also brings the organize-state cache up to date, so the
    // sidebar badge counts what the tabs count.
    let cache = library.organize_state().cloned();
    blocking(move || {
        let preview = match cache {
            Some(cache) => cache.preview(&request)?,
            None => service.preview(&request)?,
        };
        Ok(preview.into())
    })
    .await
}

/// How an item ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppliedStatusDto {
    Done,
    Failed,
    Canceled,
}

/// One item of an apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AppliedItemDto {
    pub id: String,
    pub title: String,
    pub status: AppliedStatusDto,
    /// Why it failed, for `failed`.
    pub error: String,
    pub moved: u32,
    pub copied: u32,
    pub trashed: u32,
    pub notes: Vec<String>,
}

/// The result of an apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AppliedDto {
    /// Pass it to `organize_undo`. `None` when nothing ran.
    pub batch_id: Option<String>,
    pub done: u32,
    pub failed: u32,
    pub items: Vec<AppliedItemDto>,
    /// Checked ids that were not run because they are no longer a clean change.
    pub not_run: Vec<String>,
}

impl From<Applied> for AppliedDto {
    fn from(applied: Applied) -> Self {
        let done = u32::try_from(applied.done()).unwrap_or(u32::MAX);
        let failed = u32::try_from(applied.failed()).unwrap_or(u32::MAX);
        Self {
            batch_id: applied.batch_id,
            done,
            failed,
            items: applied
                .items
                .into_iter()
                .map(|i| {
                    let (status, error) = match i.status {
                        AppliedStatus::Done => (AppliedStatusDto::Done, String::new()),
                        AppliedStatus::Failed(why) => (AppliedStatusDto::Failed, why),
                        AppliedStatus::Canceled => (AppliedStatusDto::Canceled, String::new()),
                    };
                    AppliedItemDto {
                        id: i.id,
                        title: i.title,
                        status,
                        error,
                        moved: i.moved,
                        copied: i.copied,
                        trashed: i.trashed,
                        notes: i.notes,
                    }
                })
                .collect(),
            not_run: applied.not_run,
        }
    }
}

/// Runs the checked changes of the request and records one journal batch. The request is
/// planned again first; a checked item that is not a clean change any more is not run and is
/// listed in `not_run`.
pub async fn organize_apply(
    library: &LibraryState,
    organize: &OrganizeState,
    settings: &SettingsState,
    request: OrganizeRequestDto,
    include: Vec<String>,
) -> Result<AppliedDto, OrganizeCommandError> {
    let service = organize.service(library)?;
    let preview = to_request(settings, request)?;
    blocking(move || {
        Ok(service
            .apply(&organize::ApplyRequest { preview, include })?
            .into())
    })
    .await
}

// ---- journal and undo ----------------------------------------------------------------------

/// One batch of the journal.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct JournalBatchDto {
    pub batch_id: String,
    /// When the first operation was written, epoch milliseconds.
    pub created_ms: f64,
    /// Items organized.
    pub items: u32,
    pub operations: u32,
    pub done: u32,
    pub undone: u32,
    pub failed: u32,
    /// Something is left to take back.
    pub undoable: bool,
    /// Every operation was taken back already.
    pub undone_all: bool,
    /// The titles the batch covered, in the order it reached them.
    pub titles: Vec<String>,
    /// The files it moved or copied, as the Organize footer counted them; `0` when not known
    /// (a move on one volume renames the title folder only).
    pub files: u32,
    /// When it was taken back last, epoch milliseconds; `null` when nothing of it was.
    pub undone_ms: Option<f64>,
}

#[allow(clippy::cast_precision_loss)]
impl From<JournalBatch> for JournalBatchDto {
    fn from(batch: JournalBatch) -> Self {
        let undoable = batch.undoable();
        Self {
            files: batch.files,
            undone_ms: batch.undone_ms.map(|ms| ms as f64),
            undone_all: batch.done == 0 && batch.undone > 0,
            created_ms: batch.created_ms as f64,
            batch_id: batch.batch_id,
            items: batch.items,
            operations: batch.operations,
            done: batch.done,
            undone: batch.undone,
            failed: batch.failed,
            undoable,
            titles: batch.titles,
        }
    }
}

/// The organize batches, newest first.
pub async fn organize_journal(
    library: &LibraryState,
    organize: &OrganizeState,
) -> Result<Vec<JournalBatchDto>, OrganizeCommandError> {
    let service = organize.service(library)?;
    blocking(move || {
        Ok(service
            .journal()?
            .into_iter()
            .map(JournalBatchDto::from)
            .collect())
    })
    .await
}

/// An item an undo could not restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NotRestorableDto {
    pub title: String,
    pub path: String,
    pub reason: String,
}

/// The result of an undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct UndoOutcomeDto {
    pub batch_id: String,
    /// Items fully taken back.
    pub restored: u32,
    /// Items left as they are because they changed since, each with its reason.
    pub not_restorable: Vec<NotRestorableDto>,
    /// Items the disk failed on during the undo.
    pub failed: Vec<NotRestorableDto>,
}

impl From<UndoOutcome> for UndoOutcomeDto {
    fn from(outcome: UndoOutcome) -> Self {
        let list = |items: Vec<organize::NotRestorable>| {
            items
                .into_iter()
                .map(|n| NotRestorableDto {
                    title: n.title,
                    path: n.path,
                    reason: n.reason,
                })
                .collect()
        };
        Self {
            batch_id: outcome.batch_id,
            restored: outcome.restored,
            not_restorable: list(outcome.not_restorable),
            failed: list(outcome.failed),
        }
    }
}

/// Takes a batch back in reverse. Items changed since are left alone and reported.
pub async fn organize_undo(
    library: &LibraryState,
    organize: &OrganizeState,
    batch_id: String,
) -> Result<UndoOutcomeDto, OrganizeCommandError> {
    let service = organize.service(library)?;
    blocking(move || Ok(service.undo(&batch_id)?.into())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_dto_carries_the_templates_and_whether_it_is_a_preset() {
        let settings = RenameSettings::default();
        let dto = profiles_dto(&settings);
        assert_eq!(dto.active, "clean");
        let clean = dto.profiles.iter().find(|p| p.id == "clean").unwrap();
        assert!(clean.builtin);
        assert_eq!(clean.patterns.movie_folder, "${title} (${year})");
        assert_eq!(dto.profiles.len(), 3);
    }

    #[test]
    fn a_template_check_reports_positions_in_utf16_units() {
        let findings =
            organize_check_template(PatternFieldDto::MovieFolder, "é ${titel}".to_owned());
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].severity, "error");
        assert!(findings[0].offset >= 2);
    }

    #[test]
    fn the_token_list_has_the_common_tokens() {
        let movie = organize_tokens(false);
        assert!(movie.iter().any(|t| t.name == "title"));
        assert!(organize_tokens(true).iter().any(|t| t.name == "episodeNr2"));
    }

    #[test]
    fn an_item_id_must_be_a_number() {
        assert_eq!(parse_id(" 12 ").unwrap(), ItemId(12));
        assert_eq!(parse_id("abc").unwrap_err().code, "invalid_id");
    }
}
