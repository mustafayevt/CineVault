//! The provider side of collections: searching a provider for a collection, fetching one, and
//! finding the collection of movies (after a scrape, or on request for a selection).

use std::collections::BTreeMap;

use cv_db::Database;
use cv_db::repo::{field_locks, get_item, update_item};
use cv_domain::{CalendarDate, ItemId, ItemKind, MetaField, id_key};
use cv_providers::model::{MediaKind, ProviderCollection, RequestOptions};
use cv_providers::{MetadataProvider, ProviderError};

use super::CollectionError;
use super::model::{Assigned, Failure, FindReport};
use super::store;
use crate::step_outcomes::{self, STEP_COLLECTIONS, StepStatus};

/// The first provider that knows collections.
pub(crate) fn collection_provider<'a>(
    providers: &[&'a dyn MetadataProvider],
) -> Result<&'a dyn MetadataProvider, CollectionError> {
    providers
        .iter()
        .copied()
        .find(|p| p.offers_collections())
        .ok_or(CollectionError::NoProvider)
}

/// One collection by its provider ID.
pub(crate) async fn fetch(
    provider: &dyn MetadataProvider,
    tmdb_set: &str,
    request: &RequestOptions,
) -> Result<ProviderCollection, ProviderError> {
    let mut ids = cv_domain::ProviderIds::new();
    ids.set(id_key::TMDB_SET, tmdb_set);
    provider.collection(&ids, request).await
}

/// What is known about the collection of one movie.
enum Placement {
    /// The collection ID.
    Known(String),
    /// The provider says the movie is in no collection.
    None,
    Locked,
    Failed(String),
}

/// Reads the movie's collection ID, asking the provider for the movie when it has none yet, and
/// remembers the ID on the movie.
async fn placement(
    db: &Database,
    provider: &dyn MetadataProvider,
    movie: ItemId,
    request: &RequestOptions,
    respect_locks: bool,
    lookup: bool,
) -> Result<Option<Placement>, CollectionError> {
    let found = db.read(|c| {
        let Some(item) = get_item(c, movie)? else {
            return Ok(None);
        };
        if item.kind() != ItemKind::Movie {
            return Ok(None);
        }
        let locked = respect_locks
            && (item.core.locked || field_locks(c, movie)?.contains(MetaField::Collection));
        Ok(Some((item, locked)))
    })?;
    let Some((item, locked)) = found else {
        return Ok(None);
    };
    if locked {
        return Ok(Some(Placement::Locked));
    }
    if let Some(id) = item
        .core
        .ids
        .get(id_key::TMDB_SET)
        .filter(|id| !id.is_empty())
    {
        return Ok(Some(Placement::Known(id.to_owned())));
    }
    if !lookup {
        return Ok(Some(Placement::None));
    }
    let ids = &item.core.ids;
    if !ids.contains(id_key::TMDB) && !ids.contains(id_key::IMDB) {
        return Ok(Some(Placement::Failed(
            "It has no TMDB or IMDb ID yet. Match it first.".to_owned(),
        )));
    }
    let details = match provider.details(MediaKind::Movie, ids, request).await {
        Ok(details) => details,
        Err(error) => return Ok(Some(Placement::Failed(error.to_string()))),
    };
    let Some(tmdb_set) = details
        .core
        .ids
        .get(id_key::TMDB_SET)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
    else {
        return Ok(Some(Placement::None));
    };
    let remembered = tmdb_set.clone();
    db.write(move |tx| {
        if let Some(mut item) = get_item(tx, movie)? {
            item.core.ids.set(id_key::TMDB_SET, remembered);
            update_item(tx, &item)?;
        }
        Ok(())
    })?;
    Ok(Some(Placement::Known(tmdb_set)))
}

/// Finds the collection of every movie in `movies` and puts the movie into it, creating the
/// collection when the library has none.
///
/// A movie that has a TMDB collection ID uses it; one that has not is looked up at the provider
/// first. Each collection is fetched once however many movies belong to it. A problem with one
/// movie or one collection is in the report and does not stop the others.
///
/// With `respect_locks`, a movie with its item lock or collection field lock on is left alone.
/// With `lookup` off, a movie without a collection ID is taken to have no collection instead of
/// being looked up (right after a scrape that has just written, or cleared, the ID).
///
/// # Errors
/// Fails when no provider knows collections or the library cannot be written.
pub(crate) async fn find_collections(
    db: &Database,
    providers: &[&dyn MetadataProvider],
    movies: &[ItemId],
    request: &RequestOptions,
    respect_locks: bool,
    lookup: bool,
    today: CalendarDate,
) -> Result<FindReport, CollectionError> {
    let provider = collection_provider(providers)?;
    let mut report = FindReport::default();
    let mut groups: BTreeMap<String, Vec<ItemId>> = BTreeMap::new();
    let mut seen = std::collections::HashSet::new();

    for movie in movies {
        if !seen.insert(*movie) {
            continue;
        }
        match placement(db, provider, *movie, request, respect_locks, lookup).await? {
            None => {}
            Some(Placement::Known(id)) => groups.entry(id).or_default().push(*movie),
            Some(Placement::None) => report.no_collection += 1,
            Some(Placement::Locked) => report.locked += 1,
            Some(Placement::Failed(message)) => report.failed.push(Failure {
                movie: *movie,
                message,
            }),
        }
    }

    for (tmdb_set, group) in groups {
        let remote = match fetch(provider, &tmdb_set, request).await {
            Ok(remote) => remote,
            Err(error) => {
                let message = error.to_string();
                report.failed.extend(group.iter().map(|movie| Failure {
                    movie: *movie,
                    message: message.clone(),
                }));
                continue;
            }
        };
        let (set, created, outcome) = store::write(db, move |tx| {
            let (set, created) = store::ensure_from_remote(tx, &remote, today)?;
            let outcome = store::assign(tx, &group, Some(set), respect_locks)?;
            Ok((set, created, outcome))
        })?;
        if created {
            report.created.push(set);
        }
        report.unchanged += outcome.unchanged;
        report.locked += outcome.locked;
        report
            .assigned
            .extend(outcome.moved.iter().map(|moved| Assigned {
                movie: moved.movie,
                set,
                previous: moved.previous,
            }));
    }
    Ok(report)
}

/// After a scrape that wrote the movie's collection field: puts the movie into its collection.
/// A problem is logged and never fails the scrape; Find collections can try again. The result is
/// stored as the movie's `collections` step outcome.
pub(crate) async fn assign_scraped_movie(
    db: &Database,
    providers: &[&dyn MetadataProvider],
    movie: ItemId,
    request: &RequestOptions,
) {
    if !providers.iter().any(|p| p.offers_collections()) {
        step_outcomes::record(
            db,
            movie,
            STEP_COLLECTIONS,
            StepStatus::Skipped,
            "No scraping provider offers collections.",
        );
        return;
    }
    let today = super::today();
    let (status, reason) = match find_collections(
        db,
        providers,
        &[movie],
        request,
        true,
        false,
        today,
    )
    .await
    {
        Ok(report) => {
            if let Some(failure) = report.failed.first() {
                tracing::warn!(movie = movie.0, message = %failure.message, "collection not assigned");
                (StepStatus::Failed, failure.message.clone())
            } else if report.locked > 0 {
                (
                    StepStatus::Skipped,
                    "The title is locked, so its collection was left alone.".to_owned(),
                )
            } else if report.no_collection > 0 {
                // The movie is no longer part of the provider collection it was in.
                let left = store::write(db, move |tx| store::leave_provider_set(tx, movie));
                match left {
                    Ok(Some(_)) => (
                        StepStatus::NotFound,
                        "The provider lists no collection for this movie, so it left its old one."
                            .to_owned(),
                    ),
                    Ok(None) => (
                        StepStatus::NotFound,
                        "The provider lists no collection for this movie.".to_owned(),
                    ),
                    Err(error) => (StepStatus::Failed, error.to_string()),
                }
            } else if report.assigned.is_empty() {
                (
                    StepStatus::Unchanged,
                    "It is in its collection already.".to_owned(),
                )
            } else {
                (StepStatus::Done, "Added to its collection.".to_owned())
            }
        }
        Err(error) => {
            tracing::warn!(movie = movie.0, %error, "collection not assigned");
            (StepStatus::Failed, error.to_string())
        }
    };
    step_outcomes::record(db, movie, STEP_COLLECTIONS, status, &reason);
}
