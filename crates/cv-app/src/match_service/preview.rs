//! The streaming scrape preview and the field presets.
//!
//! Choosing a result shows what scraping it would do, in two phases:
//!
//! 1. **The hit.** At once, the diff of the fields the search result already carries (title,
//!    year, runtime and, when it has one, the plot) against the stored title.
//! 2. **The details.** When the provider's details arrive, the diff of every field of the title,
//!    sent over the same stream; a field of phase one is replaced by the same field of phase two.
//!
//! A field row says what the stored title has, what the provider has and whether applying would
//! add or change it ([`FieldDiff`]); fields that are the same are in the list too, so the page can
//! say how many it hides. Fields that neither side has a value for are left out. A locked field
//! says so.
//!
//! The presets (Everything, Only empty, Artwork only, Custom) are turned by [`scrape_selection`]
//! into the field keys and the do-not-overwrite mode that `scrape_apply` takes, so applying goes
//! through the one scrape write path, which honours entity and field locks.

use std::sync::Arc;

use cv_db::repo::field_locks;
use cv_domain::{
    FieldChange, FieldSet, Item, ItemDetail, ItemId, ItemKind, MetaField, all_fields, compare_field,
};
use cv_providers::MetadataProvider;
use cv_providers::model::{MediaKind, RequestOptions};

use super::{
    MatchService, MatchServiceError, ServiceResult, ids_of_choice, media_kind, request_for,
};
use crate::artwork::stream::SourceAnswer;

/// How the fields are grouped on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffGroup {
    /// Titles, dates, descriptions, classification and the trailer.
    Text,
    /// Cast and crew.
    People,
    /// Ratings, the Top 250 place and the collection.
    RatingsAndIds,
    /// Every artwork type.
    Artwork,
}

impl DiffGroup {
    /// The group a field belongs to.
    #[must_use]
    pub fn of(field: MetaField) -> Self {
        match field {
            MetaField::Actors | MetaField::Crew => Self::People,
            MetaField::Rating | MetaField::Top250 | MetaField::Collection => Self::RatingsAndIds,
            MetaField::Artwork(_) => Self::Artwork,
            _ => Self::Text,
        }
    }

    /// A stable word for the page.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::People => "people",
            Self::RatingsAndIds => "ratings-and-ids",
            Self::Artwork => "artwork",
        }
    }
}

/// One field of the stored title next to the same field of the chosen result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDiff {
    /// The field key (`cv_domain::MetaField::key`), also the key to send back to apply.
    pub field: String,
    pub group: DiffGroup,
    /// What the stored title has; `None` when the field is empty.
    pub current: Option<String>,
    /// What the provider has; `None` when it has no value (which never blanks a stored one).
    pub incoming: Option<String>,
    pub change: FieldChange,
    /// The field, or the whole title, is locked: applying leaves it alone.
    pub locked: bool,
}

/// The chosen result, as the search showed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewCandidate {
    pub provider: String,
    pub provider_id: String,
    pub title: String,
    pub year: Option<u16>,
    pub runtime_minutes: Option<u32>,
    /// The plot the search result carried, if it had one.
    pub overview: Option<String>,
}

/// What the details phase answered.
#[derive(Debug, Clone, PartialEq)]
pub enum DetailsAnswer {
    /// The diff of every field with a value on either side.
    Fields(Vec<FieldDiff>),
    NeedsKey,
    KeyRejected,
    Failed {
        reason: String,
        retryable: bool,
    },
}

/// The plan of a streaming preview.
pub struct ScrapePreviewStream {
    /// The kind of the stored title.
    pub kind: ItemKind,
    /// The whole title is locked, so applying changes nothing.
    pub item_locked: bool,
    hit: Vec<FieldDiff>,
    details: Arc<Details>,
}

impl std::fmt::Debug for ScrapePreviewStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrapePreviewStream")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl ScrapePreviewStream {
    /// Phase one: the diff of what the search result carries. Ready at once.
    #[must_use]
    pub fn hit_fields(&self) -> &[FieldDiff] {
        &self.hit
    }

    /// Phase two: asks the provider for the details and diffs every field. Called once per start
    /// and once per retry.
    pub async fn details_fields(&self) -> DetailsAnswer {
        self.details.fetch().await
    }

    /// The provider the details come from, for the network lane it runs on.
    #[must_use]
    pub fn provider(&self) -> &str {
        self.details.provider.id()
    }
}

/// What phase two needs.
struct Details {
    provider: Arc<dyn MetadataProvider>,
    media: MediaKind,
    ids: cv_domain::ProviderIds,
    options: RequestOptions,
    stored: Item,
    locks: FieldSet,
}

impl Details {
    async fn fetch(&self) -> DetailsAnswer {
        match self
            .provider
            .details(self.media, &self.ids, &self.options)
            .await
        {
            Ok(incoming) => DetailsAnswer::Fields(diff_all(&self.stored, &incoming, &self.locks)),
            Err(error) => match SourceAnswer::of_error(&error) {
                SourceAnswer::NeedsKey => DetailsAnswer::NeedsKey,
                SourceAnswer::KeyRejected => DetailsAnswer::KeyRejected,
                SourceAnswer::Failed { reason, retryable } => {
                    DetailsAnswer::Failed { reason, retryable }
                }
                // The provider has no such title any more.
                SourceAnswer::Candidates(_) => DetailsAnswer::Failed {
                    reason: "The provider has no details for this title.".to_owned(),
                    retryable: false,
                },
            },
        }
    }
}

fn diff_of(
    stored: &Item,
    incoming: &Item,
    locks: &FieldSet,
    field: MetaField,
) -> Option<FieldDiff> {
    let compared = compare_field(stored, incoming, field);
    if compared.current.is_none() && compared.incoming.is_none() {
        return None;
    }
    Some(FieldDiff {
        field: field.key().to_owned(),
        group: DiffGroup::of(field),
        current: compared.current,
        incoming: compared.incoming,
        change: compared.change,
        locked: stored.core.locked || locks.contains(field),
    })
}

/// The diff of every field of the stored title's kind.
fn diff_all(stored: &Item, incoming: &Item, locks: &FieldSet) -> Vec<FieldDiff> {
    let kind = stored.kind();
    all_fields()
        .into_iter()
        .filter(|field| field.applies_to(kind) && field.is_scrapable())
        .filter_map(|field| diff_of(stored, incoming, locks, field))
        .collect()
}

/// The fields a search result carries, as a small incoming item.
fn hit_item(kind: ItemKind, candidate: &PreviewCandidate) -> Item {
    let mut item = if kind == ItemKind::Show {
        Item::show(&candidate.title)
    } else {
        Item::movie(&candidate.title)
    };
    item.core.year = candidate.year.unwrap_or_default();
    if let Some(overview) = &candidate.overview {
        item.core.plot.clone_from(overview);
    }
    let minutes = candidate.runtime_minutes.unwrap_or_default();
    match &mut item.detail {
        ItemDetail::Movie(movie) => movie.runtime_minutes = minutes,
        ItemDetail::Show(show) => show.runtime_minutes = minutes,
        _ => {}
    }
    item
}

/// The fields phase one can diff.
const HIT_FIELDS: [MetaField; 4] = [
    MetaField::Title,
    MetaField::Year,
    MetaField::Runtime,
    MetaField::Plot,
];

impl MatchService {
    /// Plans the streaming preview of a chosen result for `item_id`: phase one is computed here,
    /// phase two is asked with [`ScrapePreviewStream::details_fields`]. `language` overrides the
    /// metadata language when it is not blank.
    ///
    /// # Errors
    /// The item is gone or not a movie or a show, or the provider is unknown.
    pub fn scrape_preview_stream(
        self: &Arc<Self>,
        item_id: ItemId,
        candidate: &PreviewCandidate,
        language: &str,
    ) -> ServiceResult<ScrapePreviewStream> {
        let stored = self.item(item_id)?;
        let kind = stored.kind();
        let media = media_kind(kind)?;
        let context = self.context();
        let provider = context
            .providers
            .get(&candidate.provider)
            .cloned()
            .ok_or(MatchServiceError::NoProvider)?;
        let ids = ids_of_choice(provider.as_ref(), &candidate.provider_id)?;
        let locks = self.db.read(|conn| field_locks(conn, item_id))?;
        let incoming = hit_item(kind, candidate);
        let hit = HIT_FIELDS
            .into_iter()
            .filter_map(|field| diff_of(&stored, &incoming, &locks, field))
            // A field the result does not carry is not known yet: no row until the details come.
            .filter(|diff| diff.incoming.is_some())
            .collect();
        Ok(ScrapePreviewStream {
            kind,
            item_locked: stored.core.locked,
            hit,
            details: Arc::new(Details {
                provider,
                media,
                ids,
                options: request_for(&context.settings, language),
                stored,
                locks,
            }),
        })
    }
}

// ---- the presets -------------------------------------------------------------------------------

/// How much of a scrape to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrapePreset {
    /// Every field the provider has a value for.
    Everything,
    /// Only the fields the title has no value for yet.
    OnlyEmpty,
    /// Only the artwork.
    ArtworkOnly,
    /// The fields the user ticked.
    Custom,
}

/// What `scrape_apply` takes: the field keys to write and the do-not-overwrite mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrapeSelection {
    pub fields: Vec<String>,
    pub do_not_overwrite: bool,
}

impl ScrapeSelection {
    /// The fields of `diff` that applying this selection would write: not locked, not the same,
    /// and, with do-not-overwrite, only the ones the title has no value for.
    #[must_use]
    pub fn would_write<'a>(&self, diff: &'a [FieldDiff]) -> Vec<&'a FieldDiff> {
        diff.iter()
            .filter(|d| self.fields.contains(&d.field))
            .filter(|d| !d.locked && d.change != FieldChange::Same)
            .filter(|d| !(self.do_not_overwrite && d.current.is_some()))
            .collect()
    }
}

/// The selection a preset means for a title of `kind`. `custom` holds the field keys the user
/// ticked and is read only for [`ScrapePreset::Custom`]. Fields the kind does not have and keys
/// that name no field are dropped. Locks are not applied here: the scrape itself leaves locked
/// fields and locked titles alone.
#[must_use]
pub fn scrape_selection(
    kind: ItemKind,
    preset: ScrapePreset,
    custom: &[String],
) -> ScrapeSelection {
    let applicable = all_fields()
        .into_iter()
        .filter(|f| f.applies_to(kind) && f.is_scrapable());
    let keys = |fields: &mut dyn Iterator<Item = MetaField>| -> Vec<String> {
        fields.map(|f| f.key().to_owned()).collect()
    };
    match preset {
        ScrapePreset::Everything => ScrapeSelection {
            fields: keys(&mut applicable.into_iter()),
            do_not_overwrite: false,
        },
        ScrapePreset::OnlyEmpty => ScrapeSelection {
            fields: keys(&mut applicable.into_iter()),
            do_not_overwrite: true,
        },
        ScrapePreset::ArtworkOnly => ScrapeSelection {
            fields: keys(&mut applicable.filter(|f| matches!(f, MetaField::Artwork(_)))),
            do_not_overwrite: false,
        },
        ScrapePreset::Custom => {
            let wanted = FieldSet::from_keys(&custom.join(","));
            ScrapeSelection {
                fields: keys(&mut applicable.filter(|f| wanted.contains(*f))),
                do_not_overwrite: false,
            }
        }
    }
}
