//! The requests and answers of a batch run. Plain serialisable data: the CLI builds a
//! [`BatchRequest`] from its flags, a running app receives the same request over local IPC, and
//! both print the same [`BatchReport`] (as text or as `--json`).

use std::collections::BTreeMap;

use cv_domain::{ItemKind, SourceKind};
use serde::{Deserialize, Serialize};

/// Which library a batch works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Movie,
    /// Movie sets (collections).
    Movieset,
    Tvshow,
}

impl EntityKind {
    /// The kind of library item the batch lists.
    #[must_use]
    pub const fn item_kind(self) -> ItemKind {
        match self {
            Self::Movie => ItemKind::Movie,
            Self::Movieset => ItemKind::Set,
            Self::Tvshow => ItemKind::Show,
        }
    }

    /// The kind of data source that feeds the library.
    #[must_use]
    pub const fn source_kind(self) -> SourceKind {
        match self {
            // Sets live beside the movies and have no data source of their own.
            Self::Movie | Self::Movieset => SourceKind::Movie,
            Self::Tvshow => SourceKind::Tv,
        }
    }

    /// The name used on the command line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Movieset => "movieset",
            Self::Tvshow => "tvshow",
        }
    }
}

/// One step of a batch. The command line runs its steps in [`Step::ORDER`], whatever order the
/// flags were given in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    /// Rescan the data sources.
    Update,
    /// Identify and scrape titles.
    Scrape,
    /// Read the media info of titles that have none.
    MediaInfo,
    /// Detect the aspect ratio of the video.
    AspectRatio,
    /// Rename and tidy folders and files.
    Rename,
    /// Rewrite the NFO files.
    Nfo,
    // The steps below are reached through the app's selection actions only.
    /// Read the NFO files back into the library.
    ReadNfo,
    /// Calculate file checksums (CRC-32, perceptual hash).
    Checksum,
    /// Fetch ratings from the rating sources.
    Ratings,
    /// Assign movies to their collections.
    MovieSet,
    /// Download missing artwork.
    Artwork,
    /// Remove artwork the naming no longer uses.
    ArtworkCleanup,
    /// Remove unwanted files.
    Cleanup,
}

impl Step {
    /// Every step, in execution order.
    pub const ORDER: [Step; 6] = [
        Step::Update,
        Step::Scrape,
        Step::MediaInfo,
        Step::AspectRatio,
        Step::Rename,
        Step::Nfo,
    ];

    /// The step's name in reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Scrape => "scrape",
            Self::MediaInfo => "media-info",
            Self::AspectRatio => "aspect-ratio",
            Self::Rename => "rename",
            Self::Nfo => "nfo",
            Self::ReadNfo => "read-nfo",
            Self::Checksum => "checksum",
            Self::Ratings => "ratings",
            Self::MovieSet => "movie-set",
            Self::Artwork => "artwork",
            Self::ArtworkCleanup => "artwork-cleanup",
            Self::Cleanup => "cleanup",
        }
    }
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Which titles a step works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// The titles this run's update found. Without an update step in the run: the titles the
    /// library flags as NEW.
    New,
    /// The titles no provider has scraped yet.
    Unscraped,
    /// Every selected title.
    All,
}

impl Scope {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Unscraped => "unscraped",
            Self::All => "all",
        }
    }
}

/// The steps a batch runs. An empty field is a step that is not asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Steps {
    /// Rescan the selected data sources (all of them without a selection).
    #[serde(default)]
    pub update: bool,
    #[serde(default)]
    pub scrape: Option<Scope>,
    #[serde(default)]
    pub media_info: bool,
    #[serde(default)]
    pub aspect_ratio: Option<Scope>,
    #[serde(default)]
    pub rename: Option<Scope>,
    #[serde(default)]
    pub nfo: bool,
}

impl Steps {
    /// The steps asked for, in the fixed execution order: update, scrape, media info, aspect
    /// ratio, rename, NFO.
    #[must_use]
    pub fn ordered(&self) -> Vec<Step> {
        Step::ORDER
            .into_iter()
            .filter(|step| match step {
                Step::Update => self.update,
                Step::Scrape => self.scrape.is_some(),
                Step::MediaInfo => self.media_info,
                Step::AspectRatio => self.aspect_ratio.is_some(),
                Step::Rename => self.rename.is_some(),
                Step::Nfo => self.nfo,
                // The selection-action steps are not part of a command-line request.
                _ => false,
            })
            .collect()
    }

    /// No step is asked for.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ordered().is_empty()
    }
}

/// Which titles a batch works on. Every field narrows the others; with none set, the whole
/// library of the kind is selected.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionSpec {
    /// Data sources: the 1-based position in the settings order of that library, or the path.
    #[serde(default)]
    pub sources: Vec<String>,
    /// Folders (full paths): the titles inside them.
    #[serde(default)]
    pub paths: Vec<String>,
    /// A quick-search query, such as `hdr:dv year:2010..2019 -watched`.
    #[serde(default)]
    pub query: Option<String>,
    /// Exactly these titles (library item ids), in the order the library lists them. The app's
    /// selection actions use this; the command line does not.
    #[serde(default)]
    pub ids: Vec<i64>,
}

impl SelectionSpec {
    /// Nothing narrows the library.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
            && self.paths.is_empty()
            && self.query.is_none()
            && self.ids.is_empty()
    }
}

/// What a step is told besides its titles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepOptions {
    /// The arguments of a selection action (`force`, `redownload` ...), by name.
    pub args: BTreeMap<String, String>,
}

impl StepOptions {
    /// An argument of the command, if the client sent it.
    #[must_use]
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.args.get(name).map(String::as_str)
    }

    /// A true/false argument; `default` when it was not sent.
    #[must_use]
    pub fn flag(&self, name: &str, default: bool) -> bool {
        self.arg(name).map_or(default, |v| v == "true")
    }
}

/// One step of a [`Plan`] with its own titles. The command line gives every step the same
/// selection; a selection action has a selection of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedStep {
    pub step: Step,
    /// Which of the selected titles the step works on. `None`: all of them (media info, NFO and
    /// export of a command-line run work on the whole selection).
    pub scope: Option<Scope>,
    pub selection: SelectionSpec,
    /// Only titles with one of these names (movie sets picked by name). Empty: no filter.
    pub titles: Vec<String>,
    pub options: StepOptions,
}

impl PlannedStep {
    #[must_use]
    pub fn new(step: Step) -> Self {
        Self {
            step,
            scope: None,
            selection: SelectionSpec::default(),
            titles: Vec::new(),
            options: StepOptions::default(),
        }
    }
}

/// The steps a batch runs, in the order they run. A command-line request and a selection action
/// both end up as a plan, and one engine runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub kind: EntityKind,
    pub dry_run: bool,
    pub steps: Vec<PlannedStep>,
}

/// A batch to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRequest {
    pub kind: EntityKind,
    pub steps: Steps,
    #[serde(default)]
    pub selection: SelectionSpec,
    /// Plan and report, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

impl BatchRequest {
    /// The plan of a command-line request: its steps in the fixed order, all on the request's
    /// selection.
    #[must_use]
    pub fn plan(&self) -> Plan {
        let steps = &self.steps;
        let planned = steps
            .ordered()
            .into_iter()
            .map(|step| {
                let mut planned = PlannedStep::new(step);
                planned.selection = self.selection.clone();
                match step {
                    Step::Scrape => planned.scope = steps.scrape,
                    Step::AspectRatio => planned.scope = steps.aspect_ratio,
                    Step::Rename => planned.scope = steps.rename,
                    _ => {}
                }
                planned
            })
            .collect();
        Plan {
            kind: self.kind,
            dry_run: self.dry_run,
            steps: planned,
        }
    }

    #[must_use]
    pub fn new(kind: EntityKind) -> Self {
        Self {
            kind,
            steps: Steps::default(),
            selection: SelectionSpec::default(),
            dry_run: false,
        }
    }
}

// ---- the answer ----------------------------------------------------------------------------

/// How a step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// The step ran.
    Done,
    /// Dry run: what the step would do.
    Planned,
    /// There was nothing to do.
    Skipped,
    /// The step cannot run here (a tool or a feature is missing).
    Unavailable,
    /// The step stopped on an error.
    Failed,
}

/// What happened to one title in a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemResult {
    /// The library id.
    #[serde(default)]
    pub id: Option<i64>,
    pub title: String,
    /// `0` when the title has no year.
    #[serde(default)]
    pub year: u16,
    /// A word for the outcome, such as `renamed`, `matched` or `conflict`.
    pub status: String,
    /// The folder (or file) before a rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// The folder (or file) after a rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// The video file name before and after a rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_file: Option<String>,
    /// A plain sentence when there is something to say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ItemResult {
    #[must_use]
    pub fn new(id: Option<i64>, title: impl Into<String>, year: u16, status: &str) -> Self {
        Self {
            id,
            title: title.into(),
            year,
            status: status.to_owned(),
            from: None,
            to: None,
            from_file: None,
            to_file: None,
            message: None,
        }
    }

    #[must_use]
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

/// One step of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepReport {
    pub step: Step,
    pub status: StepStatus,
    /// One plain sentence.
    pub message: String,
    /// Numbers, by name: `titles`, `renamed`, `conflicts` ...
    #[serde(default)]
    pub counts: BTreeMap<String, u64>,
    /// Per-title results.
    #[serde(default)]
    pub items: Vec<ItemResult>,
    /// Where the work can be taken back, such as the rename journal batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo: Option<String>,
}

impl StepReport {
    #[must_use]
    pub fn new(step: Step, status: StepStatus, message: impl Into<String>) -> Self {
        Self {
            step,
            status,
            message: message.into(),
            counts: BTreeMap::new(),
            items: Vec::new(),
            undo: None,
        }
    }

    #[must_use]
    pub fn count(mut self, name: &str, value: usize) -> Self {
        self.counts
            .insert(name.to_owned(), u64::try_from(value).unwrap_or(u64::MAX));
        self
    }

    /// Whether any title of the step failed.
    #[must_use]
    pub fn has_failed_items(&self) -> bool {
        self.items.iter().any(|item| item.status == "failed")
    }
}

/// How a provider's credentials are set up. It never carries a key; it is built from the
/// credential status the settings page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialSummary {
    pub provider: String,
    pub name: String,
    /// `required`, `recommended` or `optional`.
    pub importance: String,
    /// `active`, `missing` or `rejected`.
    pub state: String,
    /// The provider has everything it needs.
    pub active: bool,
}

/// The titles a batch worked on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionSummary {
    /// Titles selected.
    pub titles: usize,
    /// True when a source, path or query narrowed the library.
    pub narrowed: bool,
    /// The data sources the selection covers (all of the kind without a selection).
    pub sources: Vec<String>,
}

/// The answer to a batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchReport {
    pub kind: EntityKind,
    pub dry_run: bool,
    /// Set by the CLI when a running app executed the batch.
    #[serde(default)]
    pub forwarded: bool,
    pub selection: SelectionSummary,
    /// The steps that ran, in execution order.
    pub steps: Vec<StepReport>,
    /// The metadata providers' credential status, when the batch scrapes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<CredentialSummary>,
    /// The batch could not start at all, with the reason (a bad selection, no library).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl BatchReport {
    #[must_use]
    pub fn failed_to_start(request: &BatchRequest, message: impl Into<String>) -> Self {
        Self::failed_for(request.kind, request.dry_run, message)
    }

    /// A batch of `kind` that could not start.
    #[must_use]
    pub fn failed_for(kind: EntityKind, dry_run: bool, message: impl Into<String>) -> Self {
        Self {
            kind,
            dry_run,
            forwarded: false,
            selection: SelectionSummary::default(),
            steps: Vec::new(),
            credentials: Vec::new(),
            error: Some(message.into()),
        }
    }

    /// Every step did its work: none failed or was unavailable, and no title failed.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.error.is_none()
            && self.steps.iter().all(|step| {
                !matches!(step.status, StepStatus::Failed | StepStatus::Unavailable)
                    && !step.has_failed_items()
            })
    }

    /// The process exit code: `0` when everything worked, `1` when a step or a title failed,
    /// `2` when the batch could not start.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        if self.error.is_some() {
            2
        } else if self.succeeded() {
            0
        } else {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_run_in_the_fixed_order_whatever_the_flag_order() {
        let steps = Steps {
            nfo: true,
            rename: Some(Scope::New),
            aspect_ratio: Some(Scope::All),
            media_info: true,
            scrape: Some(Scope::New),
            update: true,
        };
        let names: Vec<_> = steps.ordered().iter().map(|s| s.name()).collect();
        assert_eq!(
            names,
            [
                "update",
                "scrape",
                "media-info",
                "aspect-ratio",
                "rename",
                "nfo"
            ]
        );
        assert!(Steps::default().is_empty());
    }

    #[test]
    fn exit_codes_tell_success_failure_and_no_start_apart() {
        let request = BatchRequest::new(EntityKind::Movie);
        let mut report = BatchReport::failed_to_start(&request, "no library");
        assert_eq!(report.exit_code(), 2);
        report.error = None;
        assert_eq!(report.exit_code(), 0);
        report.steps.push(StepReport::new(
            Step::AspectRatio,
            StepStatus::Unavailable,
            "n/a",
        ));
        assert_eq!(report.exit_code(), 1);
        report.steps.clear();
        let mut step = StepReport::new(Step::Scrape, StepStatus::Done, "ok");
        step.items
            .push(ItemResult::new(Some(1), "Heat", 1995, "failed"));
        report.steps.push(step);
        assert_eq!(report.exit_code(), 1);
    }

    #[test]
    fn requests_and_reports_survive_a_json_round_trip() {
        let mut request = BatchRequest::new(EntityKind::Tvshow);
        request.steps.update = true;
        request.selection.query = Some("hdr:dv".into());
        let text = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<BatchRequest>(&text).unwrap(),
            request
        );
        // A request from an older or hand-written client may leave fields out.
        let minimal: BatchRequest =
            serde_json::from_str(r#"{"kind":"movie","steps":{"update":true}}"#).unwrap();
        assert!(minimal.steps.update && minimal.selection.is_empty() && !minimal.dry_run);
    }
}
