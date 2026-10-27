//! The command line: what each flag asks for, and how it becomes a [`BatchRequest`].

use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use cv_app::automation::{BatchRequest, EntityKind, Scope, SelectionSpec, Steps};

const EXAMPLES: &str = "\
Examples:
  cinevault movie --update-all --scrape-new --rename-new     nightly: find, scrape and rename new movies
  cinevault movie --rename-all --dry-run --json              plan a rename as JSON, change nothing
  cinevault movie --scrape-unscraped --source 2              scrape what source 2 has not scraped yet
  cinevault tvshow --rewrite-nfo --query \"year:2020..\"       rewrite the NFO files of recent shows

The steps always run in the same order, whatever order the flags are written in:
update, scrape, media info, aspect ratio, rename, NFO.
If the CineVault app is open, it runs the batch itself, so the library has one writer.";

/// CineVault: a fast movie and TV library manager.
#[derive(Debug, Parser)]
#[command(name = "cinevault", version, about, after_help = EXAMPLES, arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run batch steps on the movie library.
    Movie(BatchArgs),
    /// Run batch steps on the TV library.
    Tvshow(BatchArgs),
    /// Show each provider's importance and whether your key is active, missing or rejected.
    Providers(ProvidersArgs),
}

#[derive(Debug, Args)]
pub struct ProvidersArgs {
    /// Print JSON.
    #[arg(long)]
    pub json: bool,
}

/// The steps, the selection and the output of one batch.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct BatchArgs {
    // ---- steps ----
    /// Rescan the data sources (the selected ones, or all).
    #[arg(short = 'u', long, help_heading = "Steps")]
    pub update_all: bool,

    /// Scrape the titles this run's update found (without an update: the titles flagged NEW).
    #[arg(short = 'n', long, group = "scrape", help_heading = "Steps")]
    pub scrape_new: bool,

    /// Scrape the titles no provider has scraped yet.
    #[arg(long, group = "scrape", help_heading = "Steps")]
    pub scrape_unscraped: bool,

    /// Scrape every selected title again, taking the best match (locked titles are skipped).
    #[arg(long, group = "scrape", help_heading = "Steps")]
    pub scrape_all: bool,

    /// Read the media info of titles that have none.
    #[arg(short = 'm', long, help_heading = "Steps")]
    pub media_info: bool,

    /// Detect the aspect ratio of new titles.
    #[arg(short = 'd', long, group = "aspect", help_heading = "Steps")]
    pub aspect_ratio_new: bool,

    /// Detect the aspect ratio of every selected title.
    #[arg(long, group = "aspect", help_heading = "Steps")]
    pub aspect_ratio_all: bool,

    /// Rename the titles this run's update found (without an update: the titles flagged NEW).
    #[arg(short = 'r', long, group = "rename", help_heading = "Steps")]
    pub rename_new: bool,

    /// Rename every selected title.
    #[arg(long, group = "rename", help_heading = "Steps")]
    pub rename_all: bool,

    /// Rewrite the NFO files of the selected titles.
    #[arg(short = 'w', long, help_heading = "Steps")]
    pub rewrite_nfo: bool,

    // ---- selection ----
    /// Only this data source: its position (1 = the first of this library) or its folder.
    #[arg(long = "source", value_name = "N|FOLDER", help_heading = "Selection")]
    pub sources: Vec<String>,

    /// Only the titles inside this folder.
    #[arg(long = "path", value_name = "FOLDER", help_heading = "Selection")]
    pub paths: Vec<PathBuf>,

    /// Only the titles this quick search finds, such as "hdr:dv year:2010..2019 -watched".
    #[arg(long, value_name = "SEARCH", help_heading = "Selection")]
    pub query: Option<String>,

    // ---- output ----
    /// Print the report as JSON: per-title results for every step.
    #[arg(long, help_heading = "Output")]
    pub json: bool,

    /// Plan every step and print what it would do. Nothing is changed.
    #[arg(long, help_heading = "Output")]
    pub dry_run: bool,
}

/// An absolute version of a path given on the command line. The app that runs the batch may
/// have another working folder.
fn absolute(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

impl BatchArgs {
    /// The batch these flags ask for.
    #[must_use]
    pub fn request(&self, kind: EntityKind) -> BatchRequest {
        let scrape = if self.scrape_new {
            Some(Scope::New)
        } else if self.scrape_unscraped {
            Some(Scope::Unscraped)
        } else if self.scrape_all {
            Some(Scope::All)
        } else {
            None
        };
        let aspect_ratio = if self.aspect_ratio_new {
            Some(Scope::New)
        } else if self.aspect_ratio_all {
            Some(Scope::All)
        } else {
            None
        };
        let rename = if self.rename_new {
            Some(Scope::New)
        } else if self.rename_all {
            Some(Scope::All)
        } else {
            None
        };
        // A source given as text that is not a position is a folder.
        let sources = self
            .sources
            .iter()
            .map(|source| {
                if source.trim().chars().all(|c| c.is_ascii_digit()) {
                    source.trim().to_owned()
                } else {
                    absolute(Path::new(source.trim()))
                }
            })
            .collect();
        BatchRequest {
            kind,
            steps: Steps {
                update: self.update_all,
                scrape,
                media_info: self.media_info,
                aspect_ratio,
                rename,
                nfo: self.rewrite_nfo,
            },
            selection: SelectionSpec {
                sources,
                paths: self.paths.iter().map(|p| absolute(p)).collect(),
                query: self.query.clone(),
                ids: Vec::new(),
            },
            dry_run: self.dry_run,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("cinevault").chain(args.iter().copied()))
    }

    fn batch(args: &[&str]) -> BatchRequest {
        match parse(args).unwrap().command {
            Command::Movie(a) => a.request(EntityKind::Movie),
            Command::Tvshow(a) => a.request(EntityKind::Tvshow),
            Command::Providers(_) => panic!("not a batch"),
        }
    }

    #[test]
    fn the_nightly_example_asks_for_update_scrape_and_rename() {
        let request = batch(&["movie", "--update-all", "--scrape-new", "--rename-new"]);
        assert!(request.steps.update);
        assert_eq!(request.steps.scrape, Some(Scope::New));
        assert_eq!(request.steps.rename, Some(Scope::New));
        assert!(!request.dry_run && request.selection.is_empty());
    }

    #[test]
    fn the_flag_order_does_not_change_the_step_order() {
        let a = batch(&["movie", "-w", "-r", "-n", "-u"]);
        let b = batch(&["movie", "-u", "-n", "-r", "-w"]);
        assert_eq!(a, b);
        let names: Vec<_> = a.steps.ordered().iter().map(|s| s.name()).collect();
        assert_eq!(names, ["update", "scrape", "rename", "nfo"]);
    }

    #[test]
    fn the_dry_run_json_example_parses() {
        let cli = parse(&["movie", "--rename-all", "--dry-run", "--json"]).unwrap();
        let Command::Movie(args) = cli.command else {
            panic!("movie")
        };
        assert!(args.dry_run && args.json);
        let request = args.request(EntityKind::Movie);
        assert_eq!(request.steps.rename, Some(Scope::All));
        assert!(request.dry_run);
    }

    #[test]
    fn selection_flags_become_the_selection() {
        let request = batch(&[
            "tvshow",
            "--rewrite-nfo",
            "--source",
            "2",
            "--query",
            "year:2020..",
            "--path",
            "shows",
        ]);
        assert_eq!(request.kind, EntityKind::Tvshow);
        assert_eq!(request.selection.sources, ["2"]);
        assert_eq!(request.selection.query.as_deref(), Some("year:2020.."));
        assert!(Path::new(&request.selection.paths[0]).is_absolute());
    }

    #[test]
    fn exclusive_flags_and_missing_companions_are_refused() {
        assert!(parse(&["movie", "--scrape-new", "--scrape-all"]).is_err());
        assert!(parse(&["movie", "--rename-new", "--rename-all"]).is_err());
        // The export flags are gone: a usage error, not a step.
        assert!(parse(&["movie", "--export"]).is_err());
        assert!(parse(&["movie", "--export-template", "x", "--export-path", "out"]).is_err());
        assert!(parse(&["movie", "--download-trailer"]).is_err());
        assert!(parse(&["movie", "--view", "Recent"]).is_err());
        assert!(parse(&["movie", "--download-subtitles"]).is_err());
        assert!(parse(&["movie", "--subtitle-language", "en"]).is_err());
        assert!(parse(&[]).is_err());
    }
}
