//! Applying an ordering to a show: the provider call, the matching and the write.

use std::sync::Arc;

use cv_db::repo::{children_of, get_item, update_item};
use cv_db::{Database, DbResult};
use cv_domain::{EpisodeGroup, EpisodeOrdering, Item, ItemDetail, ItemId, MissingEpisode};
use cv_providers::model::{MediaKind, RequestOptions};
use cv_providers::{MetadataProvider, ProviderError};
use cv_scan::episode::parse_absolute_episode;
use rusqlite::Connection;

use super::matcher::{LocalEpisode, apply_pair, match_episodes};
use super::{EpisodeOrderError, ordering_options, seasons_in};
use crate::match_service::MatchService;
use crate::matching::MatchSettings;
use crate::matching::batch::MatchProviders;

/// What applying an ordering did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderingReport {
    /// The ordering the show now uses.
    pub group: EpisodeGroup,
    /// Local episodes found in the provider's list and given its numbers.
    pub matched: usize,
    /// Local episodes the list has no counterpart for. They keep their numbers.
    pub unmatched: Vec<ItemId>,
    /// Locked episodes that were matched but left unchanged.
    pub locked: usize,
    /// Episodes the list has that the library does not.
    pub missing: usize,
    /// The show's season numbers in the ordering, ascending (`0` is the specials).
    pub seasons: Vec<u32>,
}

/// Reads and writes the library for an ordering change. Knows nothing about which provider to
/// ask; see [`EpisodeOrderService`].
#[derive(Clone)]
pub struct EpisodeOrderer {
    db: Arc<Database>,
}

impl EpisodeOrderer {
    #[must_use]
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// The show, or why it cannot have an ordering.
    fn show(&self, id: ItemId) -> Result<Item, EpisodeOrderError> {
        let item = self
            .db
            .read(|c| get_item(c, id))?
            .ok_or(EpisodeOrderError::ShowNotFound)?;
        match item.detail {
            ItemDetail::Show(_) => Ok(item),
            _ => Err(EpisodeOrderError::NotAShow),
        }
    }

    /// The orderings the show can be switched to. `offered` are the provider's orderings.
    ///
    /// # Errors
    /// The show is missing or the library cannot be read.
    pub fn options(
        &self,
        show: ItemId,
        offered: &[EpisodeOrdering],
    ) -> Result<(EpisodeGroup, Vec<EpisodeGroup>), EpisodeOrderError> {
        let item = self.show(show)?;
        let ItemDetail::Show(fields) = &item.detail else {
            return Err(EpisodeOrderError::NotAShow);
        };
        Ok((
            fields.episode_group.clone(),
            ordering_options(fields, offered),
        ))
    }

    /// Switches the show to `group`: asks `provider` for the episode list in that ordering,
    /// matches the library's episodes with it and writes the numbers. The choice is stored with
    /// them, so a show whose provider lacks the ordering stays as it was.
    ///
    /// # Errors
    /// The provider has no episodes in this ordering ([`EpisodeOrderError::NotAvailable`]) or
    /// fails, the show is missing, or the library cannot be written.
    pub async fn apply(
        &self,
        show: ItemId,
        group: &EpisodeGroup,
        provider: &dyn MetadataProvider,
        request: &RequestOptions,
    ) -> Result<OrderingReport, EpisodeOrderError> {
        let item = self.show(show)?;
        let list = match provider.episodes(&item.core.ids, group, request).await {
            Ok(list) if !list.is_empty() => list,
            Ok(_) | Err(ProviderError::NotFound(_) | ProviderError::Unsupported { .. }) => {
                return Err(EpisodeOrderError::NotAvailable(group.clone()));
            }
            Err(other) => return Err(EpisodeOrderError::Provider(other)),
        };
        self.write(show, group, list).await
    }

    /// Switches the show to the aired order without asking a provider: every episode already has
    /// its aired number, so only the choice changes.
    ///
    /// # Errors
    /// The show is missing or the library cannot be written.
    pub async fn choose_aired(&self, show: ItemId) -> Result<OrderingReport, EpisodeOrderError> {
        self.show(show)?;
        self.write(show, &EpisodeGroup::aired(), Vec::new()).await
    }

    async fn write(
        &self,
        show: ItemId,
        group: &EpisodeGroup,
        list: Vec<Item>,
    ) -> Result<OrderingReport, EpisodeOrderError> {
        let db = Arc::clone(&self.db);
        let group = group.clone();
        let report = tokio::task::spawn_blocking(move || {
            db.write(move |tx| apply_list(tx, show, &group, &list))
        })
        .await
        .map_err(|e| EpisodeOrderError::Provider(ProviderError::Other(e.to_string())))??;
        report.ok_or(EpisodeOrderError::ShowNotFound)
    }
}

/// The episodes of a show, as items.
pub(crate) fn episodes_of(conn: &Connection, show: ItemId) -> DbResult<Vec<Item>> {
    let mut episodes = Vec::new();
    for id in children_of(conn, show)? {
        if let Some(item) = get_item(conn, id)?
            && matches!(item.detail, ItemDetail::Episode(_))
        {
            episodes.push(item);
        }
    }
    Ok(episodes)
}

/// The name of the video an episode was imported from, which tells its absolute number.
pub(crate) fn file_name_of(item: &Item) -> &str {
    if !item.core.original_filename.is_empty() {
        return &item.core.original_filename;
    }
    item.core
        .media_files
        .iter()
        .find(|f| f.kind == cv_domain::MediaFileType::Video)
        .map_or("", |f| f.filename.as_str())
}

pub(crate) fn local_of(show_title: &str, item: &Item) -> Option<LocalEpisode> {
    let ItemDetail::Episode(fields) = &item.detail else {
        return None;
    };
    let name = file_name_of(item);
    Some(LocalEpisode {
        id: item.id?,
        numbers: fields.numbers.clone(),
        ids: item.core.ids.clone(),
        file_absolute: (!name.is_empty())
            .then(|| parse_absolute_episode::<&str>(name, show_title, &[]))
            .flatten(),
    })
}

pub(crate) fn missing_of(item: &Item) -> Option<MissingEpisode> {
    let ItemDetail::Episode(fields) = &item.detail else {
        return None;
    };
    Some(MissingEpisode {
        title: item.core.title.clone(),
        numbers: fields.numbers.clone(),
        first_aired: fields.first_aired,
        plot: item.core.plot.clone(),
    })
}

/// Matches and writes inside one transaction. `None` when the show is gone. An empty `list`
/// (the aired order, which needs no provider) only changes the choice.
fn apply_list(
    conn: &Connection,
    show_id: ItemId,
    group: &EpisodeGroup,
    list: &[Item],
) -> DbResult<Option<OrderingReport>> {
    let Some(mut show) = get_item(conn, show_id)? else {
        return Ok(None);
    };
    if !matches!(show.detail, ItemDetail::Show(_)) {
        return Ok(None);
    }
    let mut episodes = episodes_of(conn, show_id)?;
    let locals: Vec<LocalEpisode> = episodes
        .iter()
        .filter_map(|e| local_of(&show.core.title, e))
        .collect();

    let mut report = OrderingReport {
        group: group.clone(),
        matched: 0,
        unmatched: Vec::new(),
        locked: 0,
        missing: 0,
        seasons: Vec::new(),
    };
    let mut missing = None;
    if !list.is_empty() {
        let matching = match_episodes(&locals, list, group);
        for pair in &matching.pairs {
            let id = locals[pair.local].id;
            let Some(episode) = episodes.iter_mut().find(|e| e.id == Some(id)) else {
                continue;
            };
            report.matched += 1;
            if episode.core.locked {
                report.locked += 1;
                continue;
            }
            let ItemDetail::Episode(fields) = &mut episode.detail else {
                continue;
            };
            apply_pair(
                &mut fields.numbers,
                &mut episode.core.ids,
                &list[pair.provider],
                pair.by,
            );
            update_item(conn, episode)?;
        }
        report.unmatched = matching
            .unmatched_local
            .iter()
            .map(|&i| locals[i].id)
            .collect();
        missing = Some(
            matching
                .unmatched_provider
                .iter()
                .filter_map(|&i| missing_of(&list[i]))
                .collect::<Vec<_>>(),
        );
    }

    if let ItemDetail::Show(fields) = &mut show.detail {
        fields.episode_group = group.clone();
        if !fields.episode_groups.contains(group) {
            fields.episode_groups.push(group.clone());
        }
        if let Some(missing) = missing {
            report.missing = missing.len();
            fields.missing_episodes = missing;
            fields.episode_list_fetched = true;
        }
        report.seasons = seasons_in(
            fields,
            episodes.iter().filter_map(|e| match &e.detail {
                ItemDetail::Episode(f) => Some(f),
                _ => None,
            }),
        );
    }
    update_item(conn, &show)?;
    Ok(Some(report))
}

/// The show, when it is new: its ordering is still the aired default and none of its episodes
/// has been matched to a provider yet.
fn new_show(conn: &Connection, show: ItemId) -> DbResult<Option<Item>> {
    let Some(item) = get_item(conn, show)? else {
        return Ok(None);
    };
    let ItemDetail::Show(fields) = &item.detail else {
        return Ok(None);
    };
    if fields.episode_group != EpisodeGroup::aired() {
        return Ok(None);
    }
    let untouched = episodes_of(conn, show)?
        .iter()
        .all(|episode| episode.core.ids.is_empty());
    Ok(untouched.then_some(item))
}

/// The episode list in `tv.episode-order` for a show that was just scraped for the first time.
/// `None` when the setting is the aired order, the show already had an ordering or episodes
/// matched, or the provider has no list in that order (the show stays in the aired order).
pub(crate) async fn default_ordering_list(
    db: &Database,
    show: ItemId,
    provider: &dyn MetadataProvider,
    settings: &MatchSettings,
) -> Option<(EpisodeGroup, Vec<Item>)> {
    let ordering = settings.scrape.episode_order;
    if ordering == EpisodeOrdering::Aired {
        return None;
    }
    let item = db.read(|c| new_show(c, show)).ok()??;
    let group = EpisodeGroup::new(ordering, "");
    match provider
        .episodes(&item.core.ids, &group, &settings.request)
        .await
    {
        Ok(list) if !list.is_empty() => Some((group, list)),
        Ok(_) => None,
        Err(error) => {
            tracing::debug!(%error, "the default episode order is not available for this show");
            None
        }
    }
}

/// Switches a new show to the ordering [`default_ordering_list`] found. A failure is logged: the
/// show stays in the aired order.
pub(crate) fn store_default_ordering(
    db: &Database,
    show: ItemId,
    group: &EpisodeGroup,
    list: &[Item],
) {
    let (group, list) = (group.clone(), list.to_vec());
    if let Err(error) = db.write(move |tx| apply_list(tx, show, &group, &list)) {
        tracing::warn!(%error, "the default episode order could not be stored");
    }
}

/// Why no provider can give a show's episode list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoEpisodeList {
    /// No ready provider returns episodes in the ordering.
    NoProvider,
    /// A ready provider offers the ordering, but the show has no ID that provider can look the
    /// show up by: the show was never identified with it.
    NotIdentified,
}

/// Whether `provider` offers episode lists of shows in `group`. Every show provider offers the
/// aired order.
fn offers(provider: &Arc<dyn MetadataProvider>, group: &EpisodeGroup) -> bool {
    let caps = provider.capabilities();
    caps.shows
        && (group.ordering == EpisodeOrdering::Aired || caps.orderings.contains(&group.ordering))
}

/// Whether `provider` can find `show` by an ID the show has. A provider that names no lookup IDs
/// is trusted to find it.
fn finds(provider: &Arc<dyn MetadataProvider>, show: &Item) -> bool {
    let keys = provider.capabilities().lookup_ids;
    keys.is_empty()
        || keys
            .iter()
            .any(|key| show.core.ids.get(key).is_some_and(|v| !v.trim().is_empty()))
}

/// The provider to ask for `group`: the one that scraped the show if it offers the ordering and
/// can find the show, else the first ready provider that does both.
///
/// # Errors
/// [`NoEpisodeList::NoProvider`] when no ready provider offers the ordering, and
/// [`NoEpisodeList::NotIdentified`] when some do but none can find the show by its IDs (asking
/// one would only answer that it has no such show).
pub fn provider_for(
    providers: &MatchProviders,
    show: &Item,
    group: &EpisodeGroup,
) -> Result<Arc<dyn MetadataProvider>, NoEpisodeList> {
    let usable = |p: &&Arc<dyn MetadataProvider>| offers(p, group) && finds(p, show);
    if let Some(provider) = providers
        .get(&show.core.last_scraper_id)
        .filter(usable)
        .or_else(|| {
            providers
                .supporting(MediaKind::Show)
                .into_iter()
                .find(usable)
        })
    {
        return Ok(Arc::clone(provider));
    }
    let offered = providers
        .supporting(MediaKind::Show)
        .into_iter()
        .any(|p| offers(p, group));
    Err(if offered {
        NoEpisodeList::NotIdentified
    } else {
        NoEpisodeList::NoProvider
    })
}

/// [`provider_for`] without the reason.
#[must_use]
pub fn pick_provider(
    providers: &MatchProviders,
    show: &Item,
    group: &EpisodeGroup,
) -> Option<Arc<dyn MetadataProvider>> {
    provider_for(providers, show, group).ok()
}

/// Orderings for the app: chooses the provider and the request settings, then applies.
pub struct EpisodeOrderService {
    matching: Arc<MatchService>,
}

impl EpisodeOrderService {
    #[must_use]
    pub fn new(matching: Arc<MatchService>) -> Self {
        Self { matching }
    }

    /// The show's current ordering and the ones it can be switched to. The ready providers say
    /// which of DVD and absolute they can return.
    ///
    /// # Errors
    /// The show is missing or the library cannot be read.
    pub fn options(
        &self,
        show: ItemId,
    ) -> Result<(EpisodeGroup, Vec<EpisodeGroup>), EpisodeOrderError> {
        let context = self.matching.context();
        let offered: Vec<EpisodeOrdering> = context
            .providers
            .supporting(MediaKind::Show)
            .iter()
            .flat_map(|p| p.capabilities().orderings)
            .collect();
        EpisodeOrderer::new(context.db).options(show, &offered)
    }

    /// Switches the show to `group` and rematches its episodes (see [`EpisodeOrderer::apply`]).
    /// Asking for the ordering the show already has rescrapes the episode list.
    ///
    /// # Errors
    /// No provider offers the ordering, the provider does not have it for this show
    /// ([`EpisodeOrderError::NotAvailable`]), or the library cannot be written.
    pub async fn set_ordering(
        &self,
        show: ItemId,
        group: &EpisodeGroup,
    ) -> Result<OrderingReport, EpisodeOrderError> {
        let context = self.matching.context();
        let orderer = EpisodeOrderer::new(Arc::clone(&context.db));
        let item = orderer.show(show)?;
        let provider = match provider_for(&context.providers, &item, group) {
            Ok(provider) => provider,
            // Every episode has its aired number already: choosing it needs no list.
            Err(_) if group.ordering == EpisodeOrdering::Aired => {
                return orderer.choose_aired(show).await;
            }
            Err(why) => {
                return Err(EpisodeOrderError::no_list(
                    why,
                    self.matching.identification_paused(),
                ));
            }
        };
        orderer
            .apply(show, group, provider.as_ref(), &context.settings.request)
            .await
    }
}
