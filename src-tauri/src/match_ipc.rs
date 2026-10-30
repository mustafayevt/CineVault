//! The Tauri surface of the matching commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::matching`]. Each function here is
//! the thin desktop wrapper.

use tauri::State;

use crate::sources_ipc::LibraryState;

pub use cv_ipc::matching::*;

/// Queues a match batch in the background. `item_ids` is the selection for the modes that work on
/// one; for `match_new` and `match_unmatched` it narrows the library-wide set when given.
#[tauri::command]
#[specta::specta]
pub async fn start_match_batch(
    state: State<'_, MatchState>,
    mode: MatchModeDto,
    item_ids: Option<Vec<String>>,
) -> CommandResult<MatchStartedDto> {
    cv_ipc::matching::start_match_batch(&state, mode, item_ids).await
}

/// The Match selection action: identifies the selected titles in the background. The uncertain
/// ones arrive in Needs review. Resolves to how many were queued.
#[tauri::command]
#[specta::specta]
pub async fn match_library_items(
    state: State<'_, MatchState>,
    item_ids: Vec<String>,
) -> CommandResult<u32> {
    cv_ipc::matching::match_library_items(&state, item_ids).await
}

/// The pending items of the Needs review inbox, the most confident first.
#[tauri::command]
#[specta::specta]
pub async fn list_review_items(state: State<'_, MatchState>) -> CommandResult<Vec<ReviewItemDto>> {
    cv_ipc::matching::list_review_items(&state).await
}

/// How many items need review; feeds the sidebar. It is the lifecycle count: uncertain matches
/// and titles that could not be matched, so the badge, Home and the
/// Needs review page agree.
#[tauri::command]
#[specta::specta]
pub async fn count_review_items(
    state: State<'_, MatchState>,
    library: State<'_, LibraryState>,
) -> CommandResult<u32> {
    cv_ipc::matching::count_review_items(&state, &library).await
}

/// Matches the item of a review row to the candidate and scrapes it. The item leaves the queue.
#[tauri::command]
#[specta::specta]
pub async fn accept_review_candidate(
    state: State<'_, MatchState>,
    review_id: u32,
    provider: String,
    provider_id: String,
) -> CommandResult<()> {
    cv_ipc::matching::accept_review_candidate(&state, review_id, provider, provider_id).await
}

/// Accepts the best candidate of every given review row. Resolves to the rows that were accepted.
#[tauri::command]
#[specta::specta]
pub async fn accept_top_review_candidates(
    state: State<'_, MatchState>,
    review_ids: Vec<u32>,
) -> CommandResult<Vec<u32>> {
    cv_ipc::matching::accept_top_review_candidates(&state, review_ids).await
}

/// Marks the item "not a catalogued title". Scans never queue it again.
#[tauri::command]
#[specta::specta]
pub async fn ignore_review_item(state: State<'_, MatchState>, review_id: u32) -> CommandResult<()> {
    cv_ipc::matching::ignore_review_item(&state, review_id).await
}

/// Puts accepted or ignored rows back in the queue (the Undo of both). An item accepted in this
/// session is put back as it was before the scrape; its files on disk stay.
#[tauri::command]
#[specta::specta]
pub async fn reopen_review_items(
    state: State<'_, MatchState>,
    review_ids: Vec<u32>,
) -> CommandResult<()> {
    cv_ipc::matching::reopen_review_items(&state, review_ids).await
}

/// Searches by title or by an ID shortcut for the item of a review row, with the usual scoring.
#[tauri::command]
#[specta::specta]
pub async fn search_review_candidates(
    state: State<'_, MatchState>,
    review_id: u32,
    query: String,
) -> CommandResult<Vec<ReviewCandidateDto>> {
    cv_ipc::matching::search_review_candidates(&state, review_id, query).await
}

/// Every metadata provider with whether it is ready, the Kodi add-ons that cannot be used and
/// the fields the universal provider combines. The choices themselves are the `providers.*`
/// settings.
#[tauri::command]
#[specta::specta]
pub async fn list_metadata_providers(
    state: State<'_, MatchState>,
) -> CommandResult<ProviderCatalogueDto> {
    cv_ipc::matching::list_metadata_providers(&state).await
}

/// Links the item to the result and writes the chosen fields. Locks and do-not-overwrite are
/// honoured. The answer carries the token that undoes the scrape.
#[tauri::command]
#[specta::specta]
pub async fn apply_scrape(
    state: State<'_, MatchState>,
    item_id: String,
    request: ScrapeRequestDto,
) -> CommandResult<ScrapeAppliedDto> {
    cv_ipc::matching::apply_scrape(&state, item_id, request).await
}

/// Searches for a New title and scores the results. Nothing is written: no title, NFO, artwork or
/// stage changes. An empty `query` uses the title's own name and year.
#[tauri::command]
#[specta::specta]
pub async fn search_new_title(
    state: State<'_, MatchState>,
    item_id: String,
    query: String,
    language: String,
) -> CommandResult<SearchOnlyDto> {
    cv_ipc::matching::search_new_title(&state, item_id, query, language).await
}

/// The title the item is linked to, scored like the search results, for the match picker to pin
/// first. `None` when the title has no ID the provider can look up.
#[tauri::command]
#[specta::specta]
pub async fn current_match_candidate(
    state: State<'_, MatchState>,
    item_id: String,
    language: String,
) -> CommandResult<Option<ReviewCandidateDto>> {
    cv_ipc::matching::current_match_candidate(&state, item_id, language).await
}

/// Scrapes every given title that has a confident top candidate (90 percent or more, not tied)
/// with that candidate. Runs only when the user asks.
#[tauri::command]
#[specta::specta]
pub async fn accept_confident_titles(
    state: State<'_, MatchState>,
    item_ids: Vec<String>,
) -> CommandResult<AcceptConfidentDto> {
    cv_ipc::matching::accept_confident_titles(&state, item_ids).await
}

/// Puts a title back as it was before a scrape from the workspace. Files already written stay.
#[tauri::command]
#[specta::specta]
pub async fn undo_scrape(state: State<'_, MatchState>, token: String) -> CommandResult<()> {
    cv_ipc::matching::undo_scrape(&state, token).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use async_trait::async_trait;
    use cv_app::data_location::{DataLocation, LocationInputs};
    use cv_app::match_service::CandidateView;
    use cv_app::provider_credentials::ProviderCredentialsService;
    use cv_app::secrets::SecretStore;
    use cv_app::services::{AppServices, BootOptions};
    use cv_db::repo::get_item;
    use cv_domain::ItemId;
    use cv_domain::{Item, ProviderIds};
    use cv_jobs::{JobId, JobState};
    use cv_providers::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
    use cv_providers::{
        CredentialResolver, MemoryUserCredentials, MetadataProvider, Provider, ProviderError,
        ProviderRegistry,
    };
    use cv_scan::sources::{PathStyle, SourceGuard, SystemFolders};
    use std::sync::Arc;
    use tauri::Manager;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    use super::*;
    use crate::boot::Boot;
    use crate::img_ipc::ImageState;
    use crate::library_ipc::{
        LibraryCountsDto, LibraryKindDto, library_counts, list_library_window,
    };
    use crate::lifecycle_ipc::{FlagKindDto, StageDto, get_lifecycles, lifecycle_counts};
    use crate::scan_ipc::{ScanState, rescan_library};
    use crate::sources_ipc::{LibraryState, SourceKindDto, add_data_source};

    /// A provider that knows three titles and fills the IMDb ID in its details. "Alien" is one
    /// film; "Solaris" is two films with the same title.
    struct Mock;

    const TITLES: [(&str, &str, u16, u32); 3] = [
        ("1", "Alien", 1979, 117),
        ("593", "Solaris", 1972, 167),
        ("2103", "Solaris", 2002, 99),
    ];

    fn hit(tmdb: &str, title: &str, year: u16) -> SearchHit {
        let mut ids = ProviderIds::new();
        ids.set("tmdb", tmdb);
        SearchHit {
            provider: "mock".to_owned(),
            ids,
            title: title.to_owned(),
            year,
            overview: format!("About {title}."),
            poster_url: Some("https://img.invalid/p.jpg".to_owned()),
            ..SearchHit::default()
        }
    }

    #[async_trait]
    impl Provider for Mock {
        fn id(&self) -> &'static str {
            "mock"
        }
        fn name(&self) -> &'static str {
            "Mock"
        }
    }

    #[async_trait]
    impl MetadataProvider for Mock {
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                movies: true,
                shows: true,
                orderings: Vec::new(),
                lookup_ids: vec!["tmdb", "imdb"],
                best_effort: false,
            }
        }

        async fn search(
            &self,
            query: &SearchQuery,
            _: &RequestOptions,
        ) -> Result<Vec<SearchHit>, ProviderError> {
            Ok(TITLES
                .iter()
                .filter(|(_, title, _, _)| title.eq_ignore_ascii_case(&query.title))
                .map(|(id, title, year, _)| hit(id, title, *year))
                .collect())
        }

        async fn lookup(
            &self,
            _: MediaKind,
            ids: &ProviderIds,
            _: &RequestOptions,
        ) -> Result<Option<SearchHit>, ProviderError> {
            let wanted = ids.get("tmdb").map(str::to_owned).or_else(|| {
                ids.get("imdb")
                    .map(|imdb| imdb.trim_start_matches("tt0").to_owned())
            });
            Ok(TITLES
                .iter()
                .find(|(id, ..)| {
                    wanted
                        .as_deref()
                        .is_some_and(|w| w.trim_start_matches('0') == *id)
                })
                .map(|(id, title, year, _)| hit(id, title, *year)))
        }

        async fn details(
            &self,
            kind: MediaKind,
            ids: &ProviderIds,
            _: &RequestOptions,
        ) -> Result<Item, ProviderError> {
            let Some((id, title, year, minutes)) =
                TITLES.iter().find(|(id, ..)| ids.get("tmdb") == Some(*id))
            else {
                return Err(ProviderError::NotFound("Mock".to_owned()));
            };
            let mut item = match kind {
                MediaKind::Movie => Item::movie(*title),
                MediaKind::Show => Item::show(*title),
            };
            item.core.year = *year;
            item.core.plot = format!("About {title}.");
            item.core.ids.set("tmdb", id);
            item.core.ids.set("imdb", format!("tt{id:0>7}"));
            if let cv_domain::ItemDetail::Movie(movie) = &mut item.detail {
                movie.runtime_minutes = *minutes;
            }
            Ok(item)
        }
    }

    fn put(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// The app as `main` assembles it, with the mock provider instead of the real ones.
    fn app(root: &Path) -> (tauri::App<MockRuntime>, Arc<AppServices>) {
        let inputs = LocationInputs {
            exe_dir: root.join("app"),
            user_data_base: root.join("userdata"),
            user_config_base: root.join("userconfig"),
        };
        std::fs::create_dir_all(&inputs.exe_dir).unwrap();
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        let mut registry = ProviderRegistry::new();
        registry.register_metadata(Arc::new(Mock));
        let options = BootOptions::new(
            DataLocation::resolve(&inputs),
            Arc::new(SecretStore::in_memory()),
        )
        .with_providers(ProviderCredentialsService::with_resolver(
            resolver, registry,
        ));
        let services = AppServices::boot(options).expect("boot");
        services.scan().set_background_scans(false);
        let boot = Boot::for_services(Arc::clone(&services));
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        // The real guard refuses the whole application-data tree, which holds the temp folder.
        let guard = SourceGuard::new(
            PathStyle::current(),
            SystemFolders::new(PathStyle::current()),
        );
        app.manage(LibraryState::with_guard(&services, guard));
        app.manage(ImageState::from_boot(&boot));
        app.manage(ScanState::from_services(boot.services()));
        app.manage(MatchState::from_services(boot.services()));
        app.manage(boot);
        (app, services)
    }

    async fn counts(app: &tauri::App<MockRuntime>) -> LibraryCountsDto {
        library_counts(app.state::<LibraryState>(), app.state::<ImageState>())
            .await
            .expect("counts")
    }

    /// Polls until `done`, for work that runs in the background.
    async fn until<F, Fut>(what: &str, mut done: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let deadline = Instant::now() + Duration::from_secs(120);
        while !done().await {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Whether the library holds a title that the mock provider scraped.
    fn scraped_by_mock(services: &AppServices, title: &str) -> bool {
        let ids: Vec<ItemId> = (1..=50).map(ItemId).collect();
        services
            .db()
            .read(|c| cv_db::repo::match_targets::match_targets_of(c, &ids))
            .map(|rows| {
                rows.iter()
                    .any(|row| row.title == title && row.last_scraper_id == "mock")
            })
            .unwrap_or(false)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_scanned_item_is_matched_or_lands_in_needs_review_through_the_commands() {
        let temp = tempfile::tempdir().unwrap();
        let (app, services) = app(temp.path());
        let media = tempfile::tempdir().unwrap();
        let movies = media.path().join("media").join("movies");
        put(&movies.join("Alien (1979)/Alien (1979).mkv"), "a");
        put(&movies.join("Solaris/Solaris.mkv"), "s");
        add_data_source(
            app.state::<LibraryState>(),
            SourceKindDto::Movie,
            movies.to_string_lossy().into_owned(),
        )
        .await
        .expect("add folder");

        // Rescan, the way the UI does. The titles it finds are not matched by themselves.
        let started = rescan_library(app.state::<ScanState>())
            .await
            .expect("rescan");
        let job = services
            .jobs()
            .get(JobId(u64::from(started.job_id)))
            .expect("the rescan job");
        assert_eq!(job.wait().await.snapshot.state, JobState::Completed);
        assert_eq!(counts(&app).await.movies, 2);
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(
            !scraped_by_mock(&services, "Alien"),
            "a scan scrapes nothing"
        );
        assert_eq!(
            count_review_items(app.state::<MatchState>(), app.state::<LibraryState>()).await,
            Ok(0)
        );

        // The user scrapes the new titles.
        start_match_batch(app.state::<MatchState>(), MatchModeDto::MatchNew, None)
            .await
            .expect("scrape the new titles");

        // Alien is certain: scraped. Solaris is two films: it waits in Needs review.
        until("the matching to finish", || async {
            scraped_by_mock(&services, "Alien")
                && count_review_items(app.state::<MatchState>(), app.state::<LibraryState>()).await
                    == Ok(1)
        })
        .await;
        assert_eq!(counts(&app).await.needs_review, 1);

        let queue = list_review_items(app.state::<MatchState>())
            .await
            .expect("queue");
        assert_eq!(queue.len(), 1);
        let entry = &queue[0];
        assert_eq!(entry.kind, MatchKindDto::Movie);
        assert!(entry.name.starts_with("Solaris"), "{}", entry.name);
        // The folder name has no year, which is what would tell the two films apart.
        assert_eq!(
            entry.reason,
            "No year in the file name (2 strong candidates)"
        );
        assert!(entry.attention);
        assert_eq!(entry.local.title, "Solaris");
        let years: Vec<_> = entry.candidates.iter().map(|c| c.year).collect();
        assert!(
            years.contains(&Some(1972)) && years.contains(&Some(2002)),
            "{years:?}"
        );
        assert!(entry.candidates.iter().all(|c| c.provider == "mock"));
        // The mock's poster address cannot be fetched here, so the cards keep their placeholder.
        assert!(entry.candidates.iter().all(|c| c.poster_key.is_none()));

        // The lifecycle says the same thing: Solaris is New and needs review, Alien is scraped, and
        // every count that shows the review number reads one figure.
        let pipeline = lifecycle_counts(app.state::<LibraryState>(), LibraryKindDto::Movies)
            .await
            .expect("lifecycle counts");
        assert_eq!(
            (
                pipeline.total,
                pipeline.new,
                pipeline.scraped + pipeline.renamed
            ),
            (2, 1, 1)
        );
        assert_eq!(
            (
                pipeline.needs_review,
                pipeline.uncertain,
                pipeline.not_matched
            ),
            (1, 1, 0)
        );
        let of_solaris = get_lifecycles(app.state::<LibraryState>(), vec![entry.item_id.clone()])
            .await
            .expect("lifecycle");
        let solaris_flow = &of_solaris[0].lifecycle;
        assert_eq!(solaris_flow.stage, StageDto::New);
        assert_eq!(solaris_flow.flags[0].kind, FlagKindDto::NeedsReview);
        assert_eq!(
            solaris_flow.flags[0].reason,
            "No year in the file name (2 strong candidates)"
        );
        assert_eq!(solaris_flow.lines.len(), 3);
        let window = list_library_window(
            app.state::<LibraryState>(),
            app.state::<ImageState>(),
            crate::library_ipc::LibraryQueryDto {
                kind: LibraryKindDto::Movies,
                sort: Vec::new(),
                search: String::new(),
                filters: Vec::new(),
                facets: Vec::new(),
                rating_source: None,
            },
            0,
            10,
        )
        .await
        .expect("window");
        assert!(window.items.iter().all(|card| card.lifecycle.is_some()));

        // Search by an ID shortcut, then accept the 1972 film.
        let found =
            search_review_candidates(app.state::<MatchState>(), entry.id, "tt0000593".to_owned())
                .await
                .expect("search");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].year, Some(1972));
        accept_review_candidate(
            app.state::<MatchState>(),
            entry.id,
            "mock".to_owned(),
            found[0].provider_id.clone(),
        )
        .await
        .expect("accept");
        assert_eq!(
            count_review_items(app.state::<MatchState>(), app.state::<LibraryState>()).await,
            Ok(0)
        );
        assert_eq!(counts(&app).await.needs_review, 0);
        let item_id: i64 = entry.item_id.parse().unwrap();
        let solaris = services
            .db()
            .read(|c| get_item(c, ItemId(item_id)))
            .unwrap()
            .unwrap();
        assert_eq!(solaris.core.ids.get("tmdb"), Some("593"));
        assert_eq!(solaris.core.last_scraper_id, "mock");

        // Undo puts it back; ignoring takes it out for good.
        reopen_review_items(app.state::<MatchState>(), vec![entry.id])
            .await
            .expect("reopen");
        assert_eq!(
            count_review_items(app.state::<MatchState>(), app.state::<LibraryState>()).await,
            Ok(1)
        );
        ignore_review_item(app.state::<MatchState>(), entry.id)
            .await
            .expect("ignore");
        assert_eq!(
            count_review_items(app.state::<MatchState>(), app.state::<LibraryState>()).await,
            Ok(0)
        );

        // The Match action on a selection says how many titles it queued.
        let queued = match_library_items(app.state::<MatchState>(), vec![entry.item_id.clone()])
            .await
            .expect("match");
        assert_eq!(queued, 1);
        let bad = match_library_items(app.state::<MatchState>(), vec!["x".to_owned()]).await;
        assert_eq!(bad.unwrap_err().code, "invalid");
        services.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_scrape_workspace_commands_apply_and_undo() {
        let temp = tempfile::tempdir().unwrap();
        let (app, services) = app(temp.path());
        let item = services
            .db()
            .write(|tx| cv_db::repo::insert_item(tx, &Item::movie("Alien")))
            .unwrap();
        let id = item.0.to_string();

        let applied = apply_scrape(
            app.state::<MatchState>(),
            id,
            ScrapeRequestDto {
                provider: "mock".to_owned(),
                provider_id: "1".to_owned(),
                language: "en".to_owned(),
                fields: vec!["plot".to_owned()],
                do_not_overwrite: false,
                second_version: false,
            },
        )
        .await
        .unwrap();
        let stored = services.db().read(|c| get_item(c, item)).unwrap().unwrap();
        assert_eq!(stored.core.plot, "About Alien.");
        assert_eq!(stored.core.ids.get("tmdb"), Some("1"));

        // The Undo token puts the title back as it was before the scrape.
        let token = applied
            .undo
            .expect("a scrape from the workspace can be undone");
        undo_scrape(app.state::<MatchState>(), token).await.unwrap();
        let restored = services.db().read(|c| get_item(c, item)).unwrap().unwrap();
        assert_eq!(restored.core.plot, "");
        assert_eq!(restored.core.ids.get("tmdb"), None);
        let again = undo_scrape(app.state::<MatchState>(), "-999".to_owned()).await;
        assert_eq!(again.unwrap_err().code, "invalid");
        services.shutdown();
    }

    fn view(poster_url: Option<&str>) -> CandidateView {
        CandidateView {
            provider: "mock".to_owned(),
            provider_id: "1".to_owned(),
            title: "Solaris".to_owned(),
            original_title: Some("Solyaris".to_owned()),
            year: Some(1972),
            director: None,
            runtime_minutes: None,
            language: None,
            overview: None,
            poster_url: poster_url.map(str::to_owned),
            confidence: 0.9,
            rank: 0.9,
            evidence: Vec::new(),
            breakdown: cv_app::match_service::BreakdownView {
                similarity: 1.0,
                year_penalty: 0.0,
                poster_penalty: 0.0,
                evidence: 0.0,
            },
        }
    }

    #[test]
    fn candidate_dtos_carry_the_cache_key_of_their_poster_and_never_the_address() {
        let mut posters = PosterKeys::new();
        posters.insert("https://img.test/a.jpg".to_owned(), "key-a".to_owned());

        let with = ReviewCandidateDto::of(view(Some("https://img.test/a.jpg")), &posters);
        assert_eq!(with.poster_key.as_deref(), Some("key-a"));
        // The original title travels with the candidate, for the Scrape workspace's row.
        assert_eq!(with.original_title.as_deref(), Some("Solyaris"));
        // Not downloaded (yet) or no poster at all: a placeholder.
        let pending = ReviewCandidateDto::of(view(Some("https://img.test/b.jpg")), &posters);
        assert_eq!(pending.poster_key, None);
        assert_eq!(
            ReviewCandidateDto::of(view(None), &posters).poster_key,
            None
        );
    }

    #[tokio::test]
    async fn without_services_every_command_says_why() {
        let state = MatchState {
            service: None,
            artwork: None,
        };
        let error = state
            .start_batch(MatchModeDto::MatchNew, None)
            .await
            .expect_err("no services");
        assert_eq!(error.code, "storage");
        assert!(error.message.contains("could not open the library"));
    }
}
