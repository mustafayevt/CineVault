//! The library side of episode scraping and the app-level service around it.

use std::collections::HashSet;
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::{self, get_item, update_item};
use cv_domain::{
    EpisodeGroup, FieldSet, Item, ItemDetail, ItemId, ItemKind, ScrapeBlock, ScrapeOptions,
    ScrapeOutcome,
};
use cv_providers::model::RequestOptions;
use cv_providers::{MetadataProvider, ProviderError};
use rusqlite::Connection;

use super::title_search::{EpisodeHit, EpisodePick, find_pick, search_titles};
use super::{AssignReport, EpisodeOutcome, EpisodeScrapeError, EpisodeScrapeReport};
use crate::episode_order::matcher::{
    LocalEpisode, MatchedBy, apply_pair, match_episodes, number_in,
};
use crate::episode_order::provider_for;
use crate::episode_order::service::{
    default_ordering_list, episodes_of, local_of, missing_of, store_default_ordering,
};
use crate::match_service::MatchService;
use crate::matching::MatchSettings;

type Result<T> = std::result::Result<T, EpisodeScrapeError>;

/// A show's episode list as the provider returned it, with the ordering it is in.
#[derive(Debug, Clone)]
pub struct FetchedEpisodes {
    pub group: EpisodeGroup,
    pub list: Vec<Item>,
}

fn show_of(conn: &Connection, id: ItemId) -> Result<Item> {
    let item = get_item(conn, id)?.ok_or(EpisodeScrapeError::ShowNotFound)?;
    match item.detail {
        ItemDetail::Show(_) => Ok(item),
        _ => Err(EpisodeScrapeError::NotAShow),
    }
}

fn group_of(show: &Item) -> EpisodeGroup {
    match &show.detail {
        ItemDetail::Show(fields) => fields.episode_group.clone(),
        _ => EpisodeGroup::aired(),
    }
}

/// A scrape that was blocked by the episode lock.
fn locked_outcome() -> ScrapeOutcome {
    ScrapeOutcome {
        blocked: Some(ScrapeBlock::ItemLocked),
        ..ScrapeOutcome::default()
    }
}

/// Reads the library with a closure that has its own error type.
fn read<T>(db: &Database, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    db.read(|c| Ok(f(c)))?
}

/// Writes the library in one transaction. The closures here check everything that can fail
/// before they write, so a domain error never leaves half a scrape behind.
async fn write<T: Send + 'static>(
    db: &Arc<Database>,
    f: impl FnOnce(&Connection) -> Result<T> + Send + 'static,
) -> Result<T> {
    let db = Arc::clone(db);
    tokio::task::spawn_blocking(move || db.write(|tx| Ok(f(tx))))
        .await
        .map_err(|e| EpisodeScrapeError::Provider(ProviderError::Other(e.to_string())))??
}

/// Asks `provider` for the episode list of `show` in the show's chosen ordering.
///
/// # Errors
/// The show is missing, the provider has no list ([`EpisodeScrapeError::NotAvailable`]) or
/// fails.
pub async fn fetch_for_show(
    db: &Database,
    show: ItemId,
    provider: &dyn MetadataProvider,
    request: &RequestOptions,
) -> Result<FetchedEpisodes> {
    let item = read(db, |c| show_of(c, show))?;
    let group = group_of(&item);
    match provider.episodes(&item.core.ids, &group, request).await {
        Ok(list) if !list.is_empty() => Ok(FetchedEpisodes { group, list }),
        Ok(_) | Err(ProviderError::NotFound(_) | ProviderError::Unsupported { .. }) => {
            Err(EpisodeScrapeError::NotAvailable)
        }
        Err(other) => Err(EpisodeScrapeError::Provider(other)),
    }
}

/// Scrapes the matched episodes of `show` (or just `only`) from `fetched`.
fn scrape_list(
    conn: &Connection,
    show_id: ItemId,
    only: Option<ItemId>,
    fetched: &FetchedEpisodes,
    options: &ScrapeOptions,
) -> Result<EpisodeScrapeReport> {
    let mut show = show_of(conn, show_id)?;
    let mut episodes = episodes_of(conn, show_id)?;
    if let Some(only) = only {
        episodes.retain(|e| e.id == Some(only));
        if episodes.is_empty() {
            return Err(EpisodeScrapeError::EpisodeNotFound);
        }
    }
    let list = &fetched.list;
    let locals: Vec<LocalEpisode> = episodes
        .iter()
        .filter_map(|e| local_of(&show.core.title, e))
        .collect();
    let matching = match_episodes(&locals, list, &fetched.group);

    let mut report = EpisodeScrapeReport {
        group: fetched.group.clone(),
        matched: 0,
        changed: 0,
        unmatched: matching
            .unmatched_local
            .iter()
            .map(|&i| locals[i].id)
            .collect(),
        locked: 0,
        field_locked: 0,
        missing: None,
        episodes: Vec::new(),
    };
    // The counts are of episodes, not files: two files of one episode (an original and its
    // REPACK) match one entry of the list and count once, as the show page counts them.
    let mut matched = HashSet::new();
    let mut changed = HashSet::new();
    let mut locked = HashSet::new();
    let mut field_locked = HashSet::new();
    for pair in &matching.pairs {
        let id = locals[pair.local].id;
        let Some(episode) = episodes.iter_mut().find(|e| e.id == Some(id)) else {
            continue;
        };
        matched.insert(pair.provider);
        let outcome = if episode.core.locked {
            locked.insert(pair.provider);
            locked_outcome()
        } else {
            if let ItemDetail::Episode(fields) = &mut episode.detail {
                apply_pair(
                    &mut fields.numbers,
                    &mut episode.core.ids,
                    &list[pair.provider],
                    pair.by,
                );
            }
            update_item(conn, episode)?;
            let outcome = repo::apply_scrape(conn, id, &list[pair.provider], options)?;
            if outcome.changed() {
                changed.insert(pair.provider);
            }
            if !outcome.locked.is_empty() {
                field_locked.insert(pair.provider);
            }
            outcome
        };
        report.episodes.push(EpisodeOutcome {
            id,
            by: pair.by,
            outcome,
        });
    }
    report.matched = matched.len();
    report.changed = changed.len();
    report.locked = locked.len();
    report.field_locked = field_locked.len();

    if only.is_none()
        && let ItemDetail::Show(fields) = &mut show.detail
    {
        let missing: Vec<_> = matching
            .unmatched_provider
            .iter()
            .filter_map(|&i| missing_of(&list[i]))
            .collect();
        report.missing = Some(missing.len());
        fields.missing_episodes = missing;
        fields.episode_list_fetched = true;
        update_item(conn, &show)?;
    }
    Ok(report)
}

/// Gives `episode_id` the numbers, IDs and selected fields of the list entry `pick` names.
fn assign_list(
    conn: &Connection,
    episode_id: ItemId,
    fetched: &FetchedEpisodes,
    pick: &EpisodePick,
    options: &ScrapeOptions,
) -> Result<AssignReport> {
    let mut episode = get_item(conn, episode_id)?.ok_or(EpisodeScrapeError::EpisodeNotFound)?;
    if !matches!(episode.detail, ItemDetail::Episode(_)) {
        return Err(EpisodeScrapeError::NotAnEpisode);
    }
    let show_id = episode.parent_id.ok_or(EpisodeScrapeError::ShowNotFound)?;
    let mut show = show_of(conn, show_id)?;
    let group = &fetched.group;
    let index = find_pick(&fetched.list, group, pick).ok_or(EpisodeScrapeError::UnknownEpisode)?;
    let chosen = &fetched.list[index];

    if episode.core.locked {
        return Ok(AssignReport {
            episode: episode_id,
            group: group.clone(),
            number: None,
            outcome: locked_outcome(),
        });
    }
    let mut number = None;
    if let ItemDetail::Episode(fields) = &mut episode.detail {
        // The numbers the file name gave were a guess (or missing): the provider's replace them.
        apply_pair(
            &mut fields.numbers,
            &mut episode.core.ids,
            chosen,
            MatchedBy::FileName,
        );
        number = number_in(&fields.numbers, group).map(|n| (n.season, n.episode));
    }
    update_item(conn, &episode)?;
    let outcome = repo::apply_scrape(conn, episode_id, chosen, options)?;

    // The episode is in the library now, so it is no longer missing.
    if let (Some(wanted), ItemDetail::Show(fields)) = (number, &mut show.detail) {
        let before = fields.missing_episodes.len();
        fields.missing_episodes.retain(|m| {
            number_in(&m.numbers, group).is_none_or(|n| (n.season, n.episode) != wanted)
        });
        if fields.missing_episodes.len() != before {
            update_item(conn, &show)?;
        }
    }
    Ok(AssignReport {
        episode: episode_id,
        group: group.clone(),
        number,
        outcome,
    })
}

/// Writes a fetched list into the library: the blocking half of a show scrape.
///
/// # Errors
/// The show is missing or the library cannot be written.
pub fn apply_fetched(
    db: &Database,
    show: ItemId,
    fetched: &FetchedEpisodes,
    options: &ScrapeOptions,
) -> Result<EpisodeScrapeReport> {
    let (fetched, options) = (fetched.clone(), options.clone());
    db.write(move |tx| Ok(scrape_list(tx, show, None, &fetched, &options)))?
}

/// Stores a fetched list; a failure is logged, not returned.
fn store_logged(
    db: &Database,
    show: ItemId,
    fetched: &FetchedEpisodes,
    options: &ScrapeOptions,
) -> Option<EpisodeScrapeReport> {
    match apply_fetched(db, show, fetched, options) {
        Ok(report) => Some(report),
        Err(error) => {
            tracing::warn!(%error, "episodes could not be stored");
            None
        }
    }
}

/// The list for a show that was just scraped; a problem is logged and gives `None`.
async fn fetch_logged(
    db: &Database,
    show: ItemId,
    provider: &dyn MetadataProvider,
    settings: &MatchSettings,
) -> Option<FetchedEpisodes> {
    match fetch_for_show(db, show, provider, &settings.request).await {
        Ok(fetched) => Some(fetched),
        Err(error) => {
            tracing::debug!(%error, provider = provider.id(), "episodes not scraped with the show");
            None
        }
    }
}

/// After a show was scraped: scrapes its episodes with the user's episode field selection (the
/// spec: scraping a show also scrapes its episodes). A show whose provider has no episode list
/// is left as it is. Nothing here fails the show scrape: a problem is logged and `None` is
/// returned.
pub async fn scrape_after_show(
    db: &Database,
    show: ItemId,
    provider: &dyn MetadataProvider,
    settings: &MatchSettings,
) -> Option<EpisodeScrapeReport> {
    if let Some((group, list)) = default_ordering_list(db, show, provider, settings).await {
        store_default_ordering(db, show, &group, &list);
    }
    let fetched = fetch_logged(db, show, provider, settings).await?;
    store_logged(
        db,
        show,
        &fetched,
        &settings.scrape.options_for(ItemKind::Episode),
    )
}

/// [`scrape_after_show`] for callers that keep the database in an `Arc`: the write runs on the
/// blocking pool, so the runtime never waits on SQLite.
pub async fn scrape_after_show_blocking(
    db: &Arc<Database>,
    show: ItemId,
    provider: &dyn MetadataProvider,
    settings: &MatchSettings,
) -> Option<EpisodeScrapeReport> {
    if let Some((group, list)) = default_ordering_list(db, show, provider, settings).await {
        let db = Arc::clone(db);
        let _ =
            tokio::task::spawn_blocking(move || store_default_ordering(&db, show, &group, &list))
                .await;
    }
    let fetched = fetch_logged(db, show, provider, settings).await?;
    let options = settings.scrape.options_for(ItemKind::Episode);
    let db = Arc::clone(db);
    tokio::task::spawn_blocking(move || store_logged(&db, show, &fetched, &options))
        .await
        .ok()
        .flatten()
}

/// Reads and writes the library for episode scraping. Knows nothing about which provider to ask;
/// see [`EpisodeScrapeService`].
#[derive(Clone)]
pub struct EpisodeScraper {
    db: Arc<Database>,
}

impl EpisodeScraper {
    #[must_use]
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Scrapes every episode of the show that the provider's list has a counterpart for, and
    /// refreshes the show's missing episodes from the list.
    ///
    /// # Errors
    /// The show is missing, the provider has no list or fails, or the library cannot be written.
    pub async fn scrape_show(
        &self,
        show: ItemId,
        provider: &dyn MetadataProvider,
        request: &RequestOptions,
        options: &ScrapeOptions,
    ) -> Result<EpisodeScrapeReport> {
        let fetched = fetch_for_show(&self.db, show, provider, request).await?;
        let options = options.clone();
        write(&self.db, move |tx| {
            scrape_list(tx, show, None, &fetched, &options)
        })
        .await
    }

    /// Scrapes one episode. The show's missing list is left alone.
    ///
    /// # Errors
    /// [`EpisodeScrapeError::NoCounterpart`] when the list has nothing for the episode by ID,
    /// number or file name (search it by title then); otherwise as
    /// [`EpisodeScraper::scrape_show`].
    pub async fn scrape_episode(
        &self,
        episode: ItemId,
        provider: &dyn MetadataProvider,
        request: &RequestOptions,
        options: &ScrapeOptions,
    ) -> Result<EpisodeOutcome> {
        let show = self.show_of_episode(episode)?;
        let fetched = fetch_for_show(&self.db, show, provider, request).await?;
        let options = options.clone();
        let report = write(&self.db, move |tx| {
            scrape_list(tx, show, Some(episode), &fetched, &options)
        })
        .await?;
        report
            .episodes
            .into_iter()
            .next()
            .ok_or(EpisodeScrapeError::NoCounterpart)
    }

    /// The entries of the show's episode list whose title fits `query`, best first.
    ///
    /// # Errors
    /// The show is missing or the provider has no list or fails.
    pub async fn search(
        &self,
        show: ItemId,
        query: &str,
        provider: &dyn MetadataProvider,
        request: &RequestOptions,
    ) -> Result<Vec<EpisodeHit>> {
        let fetched = fetch_for_show(&self.db, show, provider, request).await?;
        Ok(search_titles(&fetched.list, &fetched.group, query))
    }

    /// Assigns the list entry `pick` to `episode`: its numbers and IDs, then the selected
    /// fields. The list is fetched again, so a pick from an earlier search must still be in it.
    ///
    /// # Errors
    /// [`EpisodeScrapeError::UnknownEpisode`] when the entry is gone; otherwise as
    /// [`EpisodeScraper::scrape_show`].
    pub async fn assign(
        &self,
        episode: ItemId,
        pick: &EpisodePick,
        provider: &dyn MetadataProvider,
        request: &RequestOptions,
        options: &ScrapeOptions,
    ) -> Result<AssignReport> {
        let show = self.show_of_episode(episode)?;
        let fetched = fetch_for_show(&self.db, show, provider, request).await?;
        let (pick, options) = (pick.clone(), options.clone());
        write(&self.db, move |tx| {
            assign_list(tx, episode, &fetched, &pick, &options)
        })
        .await
    }

    /// The show an episode belongs to.
    ///
    /// # Errors
    /// The episode is missing or is not an episode.
    pub fn show_of_episode(&self, episode: ItemId) -> Result<ItemId> {
        let item = read(&self.db, |c| {
            get_item(c, episode)?.ok_or(EpisodeScrapeError::EpisodeNotFound)
        })?;
        if !matches!(item.detail, ItemDetail::Episode(_)) {
            return Err(EpisodeScrapeError::NotAnEpisode);
        }
        item.parent_id.ok_or(EpisodeScrapeError::ShowNotFound)
    }
}

/// What a caller changes for one run of the settings' episode scrape options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScrapeOverride {
    /// The fields to write instead of the settings' episode selection.
    pub fields: Option<FieldSet>,
    /// The do-not-overwrite mode instead of the settings'.
    pub do_not_overwrite: Option<bool>,
}

impl ScrapeOverride {
    /// `base` with the choices made for this run. Fields that episodes do not have are dropped.
    #[must_use]
    pub fn apply(&self, mut base: ScrapeOptions) -> ScrapeOptions {
        if let Some(fields) = &self.fields {
            base.fields = fields
                .iter()
                .filter(|f| f.applies_to(ItemKind::Episode))
                .collect();
        }
        if let Some(mode) = self.do_not_overwrite {
            base.do_not_overwrite = mode;
        }
        base
    }
}

/// What a service call needs: the scraper, the provider and the request and field settings.
struct Prepared {
    scraper: EpisodeScraper,
    provider: Arc<dyn MetadataProvider>,
    request: RequestOptions,
    options: ScrapeOptions,
}

/// Episode scraping for the app: chooses the provider and takes the language and the episode
/// field selection from the settings.
pub struct EpisodeScrapeService {
    matching: Arc<MatchService>,
}

impl EpisodeScrapeService {
    #[must_use]
    pub fn new(matching: Arc<MatchService>) -> Self {
        Self { matching }
    }

    /// The provider that scraped the show (else the first ready one that can return its
    /// ordering), the request settings and the episode scrape options (`choice` overrides the
    /// settings' selection and mode).
    fn prepare(&self, show: ItemId, choice: &ScrapeOverride) -> Result<Prepared> {
        let context = self.matching.context();
        let item = read(&context.db, |c| show_of(c, show))?;
        let provider =
            provider_for(&context.providers, &item, &group_of(&item)).map_err(|why| {
                EpisodeScrapeError::no_list(why, self.matching.identification_paused())
            })?;
        let options = choice.apply(context.settings.scrape.options_for(ItemKind::Episode));
        Ok(Prepared {
            scraper: EpisodeScraper::new(context.db),
            provider,
            request: context.settings.request,
            options,
        })
    }

    fn show_of_episode(&self, episode: ItemId) -> Result<ItemId> {
        EpisodeScraper::new(self.matching.context().db).show_of_episode(episode)
    }

    /// Scrapes the episodes of a show. `choice` overrides the settings' selection and mode for this run.
    ///
    /// # Errors
    /// See [`EpisodeScraper::scrape_show`]; [`EpisodeScrapeError::NoProvider`] when none is ready.
    pub async fn scrape_show(
        &self,
        show: ItemId,
        choice: &ScrapeOverride,
    ) -> Result<EpisodeScrapeReport> {
        let p = self.prepare(show, choice)?;
        p.scraper
            .scrape_show(show, p.provider.as_ref(), &p.request, &p.options)
            .await
    }

    /// Scrapes one episode.
    ///
    /// # Errors
    /// See [`EpisodeScraper::scrape_episode`].
    pub async fn scrape_episode(
        &self,
        episode: ItemId,
        choice: &ScrapeOverride,
    ) -> Result<EpisodeOutcome> {
        let p = self.prepare(self.show_of_episode(episode)?, choice)?;
        p.scraper
            .scrape_episode(episode, p.provider.as_ref(), &p.request, &p.options)
            .await
    }

    /// Searches the show's episode list by title.
    ///
    /// # Errors
    /// See [`EpisodeScraper::search`].
    pub async fn search(&self, show: ItemId, query: &str) -> Result<Vec<EpisodeHit>> {
        let p = self.prepare(show, &ScrapeOverride::default())?;
        p.scraper
            .search(show, query, p.provider.as_ref(), &p.request)
            .await
    }

    /// Assigns a searched entry to an episode and scrapes it.
    ///
    /// # Errors
    /// See [`EpisodeScraper::assign`].
    pub async fn assign(
        &self,
        episode: ItemId,
        pick: &EpisodePick,
        choice: &ScrapeOverride,
    ) -> Result<AssignReport> {
        let p = self.prepare(self.show_of_episode(episode)?, choice)?;
        p.scraper
            .assign(episode, pick, p.provider.as_ref(), &p.request, &p.options)
            .await
    }
}
