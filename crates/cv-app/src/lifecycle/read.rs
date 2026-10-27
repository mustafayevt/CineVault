//! Gathers the facts of a list of titles from the library and derives their lifecycle.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cv_db::Database;
use cv_db::DbError;
use cv_db::repo::lifecycle::{LifecycleRow, lifecycle_rows};
use cv_db::repo::organize_state::{OrganizeStateKind, OrganizeStateRow, organize_states_for};
use cv_db::repo::step_outcome::step_outcomes_for;
use cv_domain::{ItemId, ItemKind};
use serde_json::Value;

use super::{Extras, Facts, Lifecycle, Organize, Provider, Review, derive};
use crate::actions::AfterScrape;

/// What a read needs besides the library: the settings, the active naming profile and the titles
/// a job is touching now.
#[derive(Debug, Clone, Default)]
pub struct LifecycleContext {
    /// The settings values.
    pub values: BTreeMap<String, Value>,
    /// The key of the active naming profile; organize states of another profile read as
    /// "checking".
    pub profile_key: String,
    /// Titles a job is touching now.
    pub working: HashSet<ItemId>,
}

impl LifecycleContext {
    /// The extras the settings switch on for a kind of title.
    fn extras(&self, kind: ItemKind) -> Extras {
        let after = AfterScrape::from_values(&self.values);
        let movie = kind == ItemKind::Movie;
        Extras {
            ratings: if movie {
                after.ratings_movie
            } else {
                after.ratings_show
            },
            collections: movie
                && self
                    .values
                    .get("movies.collections.auto-group")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
        }
    }
}

/// The name shown for a provider key.
fn provider_label(key: &str) -> String {
    match key {
        "tmdb" => "TMDB".to_owned(),
        "tvdb" => "TheTVDB".to_owned(),
        "imdb" => "IMDb".to_owned(),
        "trakt" => "Trakt".to_owned(),
        "anidb" => "AniDB".to_owned(),
        "tvmaze" => "TVmaze".to_owned(),
        "omdb" => "OMDb".to_owned(),
        other => other.to_owned(),
    }
}

/// The provider the title was scraped from: the last scraper when it has an ID, else the first
/// known ID of the usual providers.
fn provider_of(row: &LifecycleRow) -> Option<Provider> {
    let key = [
        row.last_scraper_id.as_str(),
        "tmdb",
        "tvdb",
        "imdb",
        "trakt",
    ]
    .into_iter()
    .find(|key| row.ids.contains_key(*key))
    .map(str::to_owned)
    .or_else(|| row.ids.keys().find(|k| *k != "tmdbSet").cloned())?;
    let id = row.ids.get(&key)?.clone();
    Some(Provider {
        label: provider_label(&key),
        id,
    })
}

fn organize_of(row: Option<&OrganizeStateRow>, key: &str) -> Organize {
    let Some(row) = row.filter(|r| r.profile_key == key) else {
        return Organize::Checking;
    };
    match row.state {
        OrganizeStateKind::Tidy => Organize::Tidy,
        OrganizeStateKind::Pending => Organize::Pending(row.reason.clone()),
        OrganizeStateKind::Conflict => Organize::Conflict(row.reason.clone()),
        OrganizeStateKind::Locked => Organize::Locked,
        OrganizeStateKind::Unplanned => Organize::Unplanned(row.reason.clone()),
    }
}

/// Reads the lifecycle of `ids`. A season or an episode takes the lifecycle of its show, and a
/// collection has none (it is absent from the answer), as does an id the library does not know.
///
/// # Errors
/// The library cannot be read.
pub fn read(
    db: &Database,
    context: &LifecycleContext,
    ids: &[ItemId],
) -> Result<BTreeMap<ItemId, Lifecycle>, DbError> {
    let wanted = ids.to_vec();
    let mut rows = db.read(move |c| lifecycle_rows(c, &wanted))?;

    // A season or an episode stands for its show: walk up until a show or movie is found.
    let mut base_of: BTreeMap<ItemId, ItemId> = BTreeMap::new();
    for _ in 0..3 {
        let missing: Vec<ItemId> = rows
            .values()
            .filter(|r| matches!(r.kind, ItemKind::Season | ItemKind::Episode))
            .filter_map(|r| r.parent_id)
            .filter(|parent| !rows.contains_key(parent))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if missing.is_empty() {
            break;
        }
        let more = db.read(move |c| lifecycle_rows(c, &missing))?;
        rows.extend(more);
    }
    for (id, row) in &rows {
        let mut base = *id;
        let mut current = row;
        for _ in 0..4 {
            if !matches!(current.kind, ItemKind::Season | ItemKind::Episode) {
                break;
            }
            let Some(parent) = current.parent_id.and_then(|p| rows.get(&p)) else {
                break;
            };
            base = parent.id;
            current = parent;
        }
        base_of.insert(*id, base);
    }

    let bases: Vec<ItemId> = base_of
        .values()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let (for_outcomes, for_states) = (bases.clone(), bases);
    let mut outcomes = db.read(move |c| step_outcomes_for(c, &for_outcomes))?;
    let states = db.read(move |c| organize_states_for(c, &for_states))?;

    let mut derived: BTreeMap<ItemId, Lifecycle> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for id in ids {
        let Some(base) = base_of.get(id).copied() else {
            continue;
        };
        if let std::collections::btree_map::Entry::Vacant(slot) = derived.entry(base) {
            let Some(row) = rows.get(&base) else {
                continue;
            };
            if !matches!(row.kind, ItemKind::Movie | ItemKind::Show) {
                continue;
            }
            let mut facts = facts_of(row, context);
            facts.organize = organize_of(states.get(&base), &context.profile_key);
            facts.outcomes = outcomes.remove(&base).unwrap_or_default();
            slot.insert(derive(&facts));
        }
        if let Some(lifecycle) = derived.get(&base) {
            out.insert(*id, lifecycle.clone());
        }
    }
    Ok(out)
}

fn facts_of(row: &LifecycleRow, context: &LifecycleContext) -> Facts {
    let mut facts = Facts::new(row.kind);
    facts.title.clone_from(&row.title);
    facts.year = row.year;
    facts.added_ms = row.date_added_ms;
    facts.updated_ms = row.updated_ms;
    facts.has_provider_id = row.ids.keys().any(|k| k != "tmdbSet");
    facts.scraped_by_provider = !row.last_scraper_id.is_empty();
    facts.has_nfo = row.has_nfo;
    facts.provider = provider_of(row);
    facts.locked = row.locked;
    facts.availability = row.availability;
    facts.review = row.review.as_ref().map(|r| Review {
        reason: r.reason.clone(),
        no_candidates: r.no_candidates,
    });
    facts.working = context.working.contains(&row.id);
    facts.extras = context.extras(row.kind);
    facts.location = location_of(row);
    facts
}

/// The title's folder by its library folder's label and the folders below the library folder:
/// `Archive › Heat (1995)`. `None` for a folder outside its library folder or without a label.
fn location_of(row: &LifecycleRow) -> Option<String> {
    let label = row.source_label.trim();
    if label.is_empty() {
        return None;
    }
    let parts = |path: &str| -> Vec<String> {
        path.split(['/', '\\'])
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let root = parts(&row.data_source);
    let names = parts(&row.path);
    let inside = !root.is_empty()
        && names.len() >= root.len()
        && root
            .iter()
            .zip(&names)
            .all(|(a, b)| a.eq_ignore_ascii_case(b));
    if !inside {
        return None;
    }
    Some(
        std::iter::once(label.to_owned())
            .chain(names[root.len()..].iter().cloned())
            .collect::<Vec<_>>()
            .join(" \u{203a} "),
    )
}
