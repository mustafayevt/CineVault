//! The streaming scrape preview and the presets: the diff from the search result
//! comes first, the details fill in the rest, "Only empty" writes only empty fields and a locked
//! field stays as it is. A mock provider, no network.

mod support;

use std::sync::Arc;

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::match_service::{
    DetailsAnswer, DiffGroup, FieldDiff, MatchService, PreviewCandidate, ScrapeApply, ScrapePreset,
    scrape_selection,
};
use cv_app::notices::NoticeHub;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_db::repo::set_field_lock;
use cv_domain::{
    Credit, CreditKind, FieldChange, Item, ItemDetail, ItemId, ItemKind, MediaFileType, MetaField,
    ProviderIds,
};
use cv_providers::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use cv_providers::{
    CredentialResolver, MemoryUserCredentials, MetadataProvider, Provider, ProviderError,
    ProviderRegistry,
};
use support::Lab;

/// Knows one film, "Heat" (1995), with a tagline, a plot, a cast and a poster.
struct Heat;

#[async_trait]
impl Provider for Heat {
    fn id(&self) -> &'static str {
        "omdb"
    }
    fn name(&self) -> &'static str {
        "OMDb"
    }
}

#[async_trait]
impl MetadataProvider for Heat {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: false,
            orderings: Vec::new(),
            lookup_ids: vec!["imdb"],
            best_effort: false,
        }
    }

    async fn search(
        &self,
        _query: &SearchQuery,
        _options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        Ok(Vec::new())
    }

    async fn lookup(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        Ok(None)
    }

    async fn details(
        &self,
        _kind: MediaKind,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        if ids.get("imdb") != Some("tt0113277") {
            return Err(ProviderError::NotFound("OMDb".to_owned()));
        }
        let mut item = Item::movie("Heat");
        item.core.year = 1995;
        item.core.plot = "A group of professional bank robbers start to feel the heat.".to_owned();
        item.core.ids.set("imdb", "tt0113277");
        item.core
            .credits
            .push(Credit::new(CreditKind::Actor, "Al Pacino", "Hanna"));
        item.core.artwork_urls.insert(
            MediaFileType::Poster,
            "https://img.test/heat.jpg".to_owned(),
        );
        if let ItemDetail::Movie(movie) = &mut item.detail {
            movie.tagline = "A Los Angeles crime saga".to_owned();
            movie.runtime_minutes = 170;
        }
        Ok(item)
    }
}

struct Rig {
    lab: Lab,
    service: Arc<MatchService>,
}

fn rig() -> Rig {
    let lab = Lab::new();
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(Arc::new(Heat));
    let credentials = Arc::new(ProviderCredentialsService::with_resolver(
        CredentialResolver::new(Arc::new(MemoryUserCredentials::default())),
        registry,
    ));
    credentials.set("omdb", "api_key", "a-key").unwrap();
    let service = MatchService::new(
        Arc::clone(&lab.db),
        lab.manager.clone(),
        credentials,
        Arc::new(SettingsService::in_memory("")),
        Arc::new(NoticeHub::new()),
    );
    Rig { lab, service }
}

fn candidate() -> PreviewCandidate {
    PreviewCandidate {
        provider: "omdb".to_owned(),
        provider_id: "tt0113277".to_owned(),
        title: "Heat".to_owned(),
        year: Some(1995),
        runtime_minutes: Some(170),
        overview: None,
    }
}

/// A film the user named loosely: a wrong title, no year, a plot of their own.
fn stored(rig: &Rig) -> ItemId {
    let mut item = Item::movie("heat dvdrip");
    item.core.plot = "My own plot".to_owned();
    rig.lab.add(item)
}

fn diff<'a>(fields: &'a [FieldDiff], key: &str) -> &'a FieldDiff {
    fields
        .iter()
        .find(|d| d.field == key)
        .unwrap_or_else(|| panic!("no {key} in {fields:?}"))
}

fn apply_request(selection: &cv_app::match_service::ScrapeSelection) -> ScrapeApply {
    ScrapeApply {
        provider: "omdb".to_owned(),
        provider_id: "tt0113277".to_owned(),
        language: "en".to_owned(),
        fields: selection.fields.clone(),
        do_not_overwrite: selection.do_not_overwrite,
        second_version: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_diff_of_the_search_result_comes_first_and_the_details_fill_in_the_rest() {
    let rig = rig();
    let item = stored(&rig);
    let stream = rig
        .service
        .scrape_preview_stream(item, &candidate(), "")
        .unwrap();

    // Phase one: what the search result carries, at once.
    let hit = stream.hit_fields();
    let keys: Vec<&str> = hit.iter().map(|d| d.field.as_str()).collect();
    assert_eq!(
        keys,
        ["title", "year", "runtime"],
        "no plot: the result had none"
    );
    assert_eq!(diff(hit, "title").change, FieldChange::Changed);
    assert_eq!(diff(hit, "title").current.as_deref(), Some("heat dvdrip"));
    assert_eq!(diff(hit, "title").incoming.as_deref(), Some("Heat"));
    assert_eq!(diff(hit, "year").change, FieldChange::Added);
    assert_eq!(diff(hit, "runtime").incoming.as_deref(), Some("170"));

    // Phase two: every field with a value on either side.
    let DetailsAnswer::Fields(all) = stream.details_fields().await else {
        panic!("the details did not arrive");
    };
    assert_eq!(diff(&all, "plot").change, FieldChange::Changed);
    assert_eq!(diff(&all, "plot").group, DiffGroup::Text);
    assert_eq!(diff(&all, "tagline").change, FieldChange::Added);
    assert_eq!(diff(&all, "actors").group, DiffGroup::People);
    assert_eq!(diff(&all, "actors").incoming.as_deref(), Some("Al Pacino"));
    assert_eq!(diff(&all, "poster").group, DiffGroup::Artwork);
    assert_eq!(
        diff(&all, "poster").incoming.as_deref(),
        Some("https://img.test/heat.jpg")
    );
    // The fields of phase one are in phase two as well, so the page replaces them by key.
    for key in keys {
        assert!(all.iter().any(|d| d.field == key), "{key}");
    }
    // A field neither side has a value for is not listed.
    assert!(all.iter().all(|d| d.field != "studio"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_title_the_provider_does_not_know_fails_with_a_reason() {
    let rig = rig();
    let item = stored(&rig);
    let mut unknown = candidate();
    unknown.provider_id = "tt9999999".to_owned();
    let stream = rig
        .service
        .scrape_preview_stream(item, &unknown, "")
        .unwrap();

    let DetailsAnswer::Failed { reason, retryable } = stream.details_fields().await else {
        panic!("expected a failure");
    };
    assert!(reason.contains("no details"), "{reason}");
    assert!(!retryable);
    // The first phase is still there.
    assert!(!stream.hit_fields().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_empty_fills_the_empty_fields_and_keeps_the_plot() {
    let rig = rig();
    let item = stored(&rig);
    let selection = scrape_selection(ItemKind::Movie, ScrapePreset::OnlyEmpty, &[]);
    assert!(selection.do_not_overwrite);

    // What the preview says it would do ...
    let stream = rig
        .service
        .scrape_preview_stream(item, &candidate(), "")
        .unwrap();
    let DetailsAnswer::Fields(all) = stream.details_fields().await else {
        panic!()
    };
    let would: Vec<&str> = selection
        .would_write(&all)
        .iter()
        .map(|d| d.field.as_str())
        .collect();
    assert!(
        would.contains(&"tagline") && would.contains(&"year"),
        "{would:?}"
    );
    assert!(
        !would.contains(&"plot") && !would.contains(&"title"),
        "{would:?}"
    );

    // ... is what applying does.
    rig.service
        .scrape_apply(item, &apply_request(&selection))
        .await
        .unwrap();
    let after = rig.lab.item(item);
    assert_eq!(after.core.plot, "My own plot", "a plot that is there stays");
    assert_eq!(after.core.title, "heat dvdrip");
    assert_eq!(after.core.year, 1995, "an empty year is filled");
    let ItemDetail::Movie(movie) = &after.detail else {
        panic!()
    };
    assert_eq!(movie.tagline, "A Los Angeles crime saga");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_locked_field_is_flagged_and_stays_as_it_is_with_every_preset() {
    let rig = rig();
    let item = stored(&rig);
    rig.lab
        .db
        .write(move |tx| set_field_lock(tx, item, MetaField::Plot, true))
        .unwrap();
    let selection = scrape_selection(ItemKind::Movie, ScrapePreset::Everything, &[]);

    let stream = rig
        .service
        .scrape_preview_stream(item, &candidate(), "")
        .unwrap();
    let DetailsAnswer::Fields(all) = stream.details_fields().await else {
        panic!()
    };
    assert!(diff(&all, "plot").locked);
    assert!(!diff(&all, "title").locked);
    let would: Vec<&str> = selection
        .would_write(&all)
        .iter()
        .map(|d| d.field.as_str())
        .collect();
    assert!(!would.contains(&"plot"), "{would:?}");

    rig.service
        .scrape_apply(item, &apply_request(&selection))
        .await
        .unwrap();
    let after = rig.lab.item(item);
    assert_eq!(
        after.core.plot, "My own plot",
        "the locked plot is untouched"
    );
    assert_eq!(after.core.title, "Heat", "everything else was written");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_locked_title_is_flagged_on_every_field() {
    let rig = rig();
    let mut item = Item::movie("heat dvdrip");
    item.core.locked = true;
    let item = rig.lab.add(item);

    let stream = rig
        .service
        .scrape_preview_stream(item, &candidate(), "")
        .unwrap();

    assert!(stream.item_locked);
    assert!(stream.hit_fields().iter().all(|d| d.locked));
}

#[test]
fn the_presets_map_onto_the_selectable_fields() {
    let everything = scrape_selection(ItemKind::Movie, ScrapePreset::Everything, &[]);
    assert!(!everything.do_not_overwrite);
    assert!(everything.fields.contains(&"plot".to_owned()));
    assert!(everything.fields.contains(&"poster".to_owned()));
    // A movie has no broadcast status; a show has.
    assert!(!everything.fields.contains(&"status".to_owned()));
    let show = scrape_selection(ItemKind::Show, ScrapePreset::Everything, &[]);
    assert!(show.fields.contains(&"status".to_owned()));

    let artwork = scrape_selection(
        ItemKind::Movie,
        ScrapePreset::ArtworkOnly,
        &["plot".to_owned()],
    );
    assert!(!artwork.fields.is_empty() && !artwork.do_not_overwrite);
    assert!(artwork.fields.contains(&"fanart".to_owned()));
    assert!(!artwork.fields.contains(&"plot".to_owned()));

    // Custom keeps only the ticked fields that exist for the kind.
    let custom = scrape_selection(
        ItemKind::Movie,
        ScrapePreset::Custom,
        &[
            "rating".to_owned(),
            "fanart".to_owned(),
            "status".to_owned(),
            "no-such-field".to_owned(),
        ],
    );
    assert_eq!(custom.fields, ["rating", "fanart"]);
    assert!(!custom.do_not_overwrite);

    // The other presets ignore what was ticked.
    let only_empty = scrape_selection(
        ItemKind::Movie,
        ScrapePreset::OnlyEmpty,
        &["plot".to_owned()],
    );
    assert_eq!(only_empty.fields, everything.fields);
}
