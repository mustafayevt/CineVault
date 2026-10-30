//! Running a parsed command line: hand the batch to an open app, or run it here.

use std::io::Write;
use std::sync::Arc;

use cv_app::automation::ipc::{ForwardError, forward};
use cv_app::automation::{
    BatchEngine, BatchReport, BatchRequest, EngineOptions, Progress, credential_summaries,
};
use cv_app::data_location::DataLocation;
use cv_app::fresh_start::FreshStartError;
use cv_app::organize::recover_interrupted_batches;
use cv_app::provider_setup::build_service;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootError, BootOptions};

use crate::args::{Cli, Command};
use crate::output;

/// What the command line runs against: the data folder, and how to build the services when no
/// app is open. Tests supply their own.
pub struct Env {
    location: DataLocation,
    make_options: Box<dyn Fn(DataLocation) -> BootOptions + Send + Sync>,
    secrets: Arc<SecretStore>,
}

impl Env {
    /// The environment of the running process: the user's data folder and the OS credential
    /// store, which holds the user's own provider keys.
    ///
    /// # Errors
    /// The user's data folder cannot be found.
    pub fn from_process() -> anyhow::Result<Self> {
        let location = DataLocation::resolve_for_process()
            .ok_or_else(|| anyhow::anyhow!("CineVault could not find a folder for its data."))?;
        let secrets = Arc::new(SecretStore::for_location(&location));
        Ok(Self::with_services(location, secrets, |options| options))
    }

    /// An environment over the given parts. `adjust` changes the boot options (tests use it to
    /// register scripted providers).
    pub fn with_services(
        location: DataLocation,
        secrets: Arc<SecretStore>,
        adjust: impl Fn(BootOptions) -> BootOptions + Send + Sync + 'static,
    ) -> Self {
        let s = Arc::clone(&secrets);
        Self {
            location,
            // Only the app makes the fresh start of a new version; the command line refuses a
            // data folder that still needs it and says so.
            make_options: Box::new(move |location| {
                adjust(BootOptions::new(location, Arc::clone(&s))).without_fresh_start()
            }),
            secrets,
        }
    }

    #[must_use]
    pub fn location(&self) -> &DataLocation {
        &self.location
    }
}

fn progress_to_stderr(progress: &Progress) {
    match progress.step {
        Some(step) => eprintln!("[{step}] {}", progress.message),
        None => eprintln!("{}", progress.message),
    }
}

/// Runs the command and prints its result to `out`. Returns the process exit code: `0` when
/// everything worked, `1` when a step or a title failed, `2` when the command could not run.
pub async fn execute(cli: Cli, env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match cli.command {
        Command::Providers(args) => {
            let service = build_service(Arc::clone(&env.secrets), None);
            let providers = credential_summaries(&service);
            let text = if args.json {
                output::providers_json(&providers)
            } else {
                output::providers_text(&providers)
            };
            let _ = writeln!(out, "{}", text.trim_end());
            0
        }
        Command::Movie(args) => {
            let request = args.request(cv_app::automation::EntityKind::Movie);
            batch(&request, args.json, env, out, err).await
        }
        Command::Tvshow(args) => {
            let request = args.request(cv_app::automation::EntityKind::Tvshow);
            batch(&request, args.json, env, out, err).await
        }
    }
}

async fn batch(
    request: &BatchRequest,
    json: bool,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let report = run_batch(request, json, env).await;
    let rendered = if json {
        output::json(&report)
    } else {
        output::text(&report)
    };
    let _ = writeln!(out, "{}", rendered.trim_end());
    if report.error.is_some() && json {
        let _ = writeln!(err, "{}", report.error.as_deref().unwrap_or_default());
    }
    report.exit_code()
}

/// Runs the batch through the open app when there is one, else here.
pub async fn run_batch(request: &BatchRequest, quiet: bool, env: &Env) -> BatchReport {
    if request.steps.is_empty() {
        return BatchReport::failed_to_start(
            request,
            "No step was asked for. Choose at least one, such as --update-all, --scrape-new or \
             --rename-all. See --help.",
        );
    }
    let on_progress = |progress: Progress| {
        if !quiet {
            progress_to_stderr(&progress);
        }
    };
    match forward(env.location(), request, on_progress).await {
        Ok(report) => return report,
        Err(ForwardError::NoRunningApp) => {
            if let Some(owner) = cv_app::owner::read_owner(env.location().root())
                && cv_app::owner::is_fresh(&owner)
                && owner.role != "cli"
            {
                return BatchReport::failed_to_start(
                    request,
                    format!(
                        "{} The batch was not run here, because the library has one writer. \
                         Stop it, or send the batch through it.",
                        owner.sentence()
                    ),
                );
            }
        }
        Err(error) => {
            return BatchReport::failed_to_start(
                request,
                format!("{error}. The batch was not run here, because the app holds the library."),
            );
        }
    }
    run_here(request, quiet, env).await
}

/// Opens the library in this process and runs the batch.
async fn run_here(request: &BatchRequest, quiet: bool, env: &Env) -> BatchReport {
    // Refuse a library of an earlier version before taking the owner file, so a refused run
    // leaves the data folder exactly as it found it. A data folder that is not a folder is left
    // to the boot below, which has the sentence for it.
    if env.location().root().is_dir()
        && let Err(error) = cv_app::fresh_start::refuse_pending(env.location())
    {
        return BatchReport::failed_to_start(request, error.to_string());
    }
    let _owner = match cv_app::owner::acquire(env.location().root(), "cli") {
        Ok(Some(guard)) => Some(guard),
        Ok(None) => {
            let current = cv_app::owner::read_owner(env.location().root());
            let why = current.map_or_else(
                || "Another CineVault process is using this library.".to_owned(),
                |owner| owner.sentence(),
            );
            return BatchReport::failed_to_start(
                request,
                format!("{why} The batch was not run here, because the library has one writer."),
            );
        }
        Err(error) => {
            // The boot below reports an unusable data folder in its own words.
            tracing::warn!(%error, "the owner file could not be taken; trying to open the library");
            None
        }
    };
    let options = (env.make_options)(env.location.clone());
    let services = match AppServices::boot(options) {
        Ok(services) => services,
        Err(error @ BootError::FreshStart(FreshStartError::NeedsApp { .. })) => {
            return BatchReport::failed_to_start(request, error.to_string());
        }
        Err(error) => {
            return BatchReport::failed_to_start(
                request,
                format!(
                    "{error}. If the CineVault app is open, close it and run the command again, \
                     or update the app so it can run command-line batches itself."
                ),
            );
        }
    };
    // Settle what a crash left half done before anything is renamed.
    if let Err(error) = recover_interrupted_batches(services.db()) {
        tracing::warn!(%error, "organize crash recovery could not run");
    }
    // A batch that finished without its History entry (a stop in between) gets one.
    if let Err(error) =
        cv_app::library_ops::RecoveryService::new(Arc::clone(services.db())).record_unrecorded()
    {
        tracing::warn!(%error, "finished batches could not be checked for History entries");
    }
    // The steps of the command line are explicit: titles a scan finds are not matched behind the
    // run's back.
    services.scan().set_after_scan(Arc::new(|_| {}));
    let engine = BatchEngine::new(Arc::clone(&services), EngineOptions::default());
    let sink = move |progress: Progress| {
        if !quiet {
            progress_to_stderr(&progress);
        }
    };
    let report = engine.run(request, &sink).await;
    services.shutdown();
    report
}
