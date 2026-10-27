//! Scrape fields, locks and the rules that apply a scraped item to a stored one.
//!
//! A [`MetaField`] names one piece of metadata a scrape may write and a user may lock. A scrape
//! is described by [`ScrapeOptions`]: the fields it may write and the "do not overwrite existing
//! data" mode. [`apply_scrape`] merges a scraped item into a stored one under those options, an
//! entity lock (`ItemCore::locked`) and the item's field locks. [`revert_locked`] is the same
//! protection for writers that are not scrapes (Read NFO, bulk refresh): after they changed an
//! item, it puts the locked fields back.
//!
//! Rules of a scrape:
//! * a locked entity is never modified;
//! * a field that is not selected, not locked-free or not applicable to the kind is left alone;
//! * a value the provider does not have never blanks a stored one;
//! * with *do not overwrite*, a field is written only when the stored field is empty;
//! * provider IDs are only ever added (missing keys), except the collection ID, which is the
//!   [`MetaField::Collection`] field.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    AiredStatus, CalendarDate, Credit, CreditKind, Genre, Item, ItemDetail, ItemKind,
    MediaFileType, Rating, Ratings, Trailer, id_key, rating_key,
};

/// One field of metadata that can be selected for a scrape and locked against it.
///
/// The order of the variants is the order of the lists in the settings and in the NFO lock
/// element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MetaField {
    Title,
    OriginalTitle,
    Tagline,
    Plot,
    Year,
    /// The release date of a movie, the first-aired date of a show or episode.
    ReleaseDate,
    /// The broadcast status of a show.
    Status,
    /// The ratings from rating sources. The user's own rating is never touched.
    Rating,
    Top250,
    Runtime,
    Certification,
    Genres,
    SpokenLanguages,
    Country,
    /// Studios / production companies.
    Studio,
    Tags,
    /// The collection (set) the movie belongs to, recorded as its provider ID.
    Collection,
    /// Cast: actors and guest stars.
    Actors,
    /// Crew: directors, writers, producers and the rest.
    Crew,
    Trailer,
    /// One artwork type.
    Artwork(MediaFileType),
}

/// The artwork types that have a field, with their keys.
const ARTWORK_KEYS: &[(MediaFileType, &str)] = &[
    (MediaFileType::Poster, "poster"),
    (MediaFileType::Fanart, "fanart"),
    (MediaFileType::Banner, "banner"),
    (MediaFileType::Clearart, "clearart"),
    (MediaFileType::Disc, "disc"),
    (MediaFileType::Clearlogo, "clearlogo"),
    (MediaFileType::Thumb, "thumb"),
    (MediaFileType::Characterart, "characterart"),
    (MediaFileType::Keyart, "keyart"),
    (MediaFileType::Squareart, "squareart"),
    (MediaFileType::SeasonPoster, "season_poster"),
    (MediaFileType::SeasonFanart, "season_fanart"),
    (MediaFileType::SeasonBanner, "season_banner"),
    (MediaFileType::SeasonThumb, "season_thumb"),
    (MediaFileType::Extrafanart, "extrafanart"),
    (MediaFileType::Extrathumb, "extrathumb"),
];

/// The non-artwork fields with their keys, in order.
const TEXT_KEYS: &[(MetaField, &str)] = &[
    (MetaField::Title, "title"),
    (MetaField::OriginalTitle, "original_title"),
    (MetaField::Tagline, "tagline"),
    (MetaField::Plot, "plot"),
    (MetaField::Year, "year"),
    (MetaField::ReleaseDate, "release_date"),
    (MetaField::Status, "status"),
    (MetaField::Rating, "rating"),
    (MetaField::Top250, "top250"),
    (MetaField::Runtime, "runtime"),
    (MetaField::Certification, "certification"),
    (MetaField::Genres, "genres"),
    (MetaField::SpokenLanguages, "spoken_languages"),
    (MetaField::Country, "country"),
    (MetaField::Studio, "studio"),
    (MetaField::Tags, "tags"),
    (MetaField::Collection, "collection"),
    (MetaField::Actors, "actors"),
    (MetaField::Crew, "crew"),
    (MetaField::Trailer, "trailer"),
];

/// Every field, in list order.
pub fn all_fields() -> Vec<MetaField> {
    TEXT_KEYS
        .iter()
        .map(|(field, _)| *field)
        .chain(
            ARTWORK_KEYS
                .iter()
                .map(|(kind, _)| MetaField::Artwork(*kind)),
        )
        .collect()
}

impl MetaField {
    /// The stable key used in settings, the lock table and the NFO lock element.
    pub fn key(self) -> &'static str {
        match self {
            Self::Artwork(kind) => ARTWORK_KEYS
                .iter()
                .find(|(k, _)| *k == kind)
                .map_or("artwork", |(_, key)| key),
            other => TEXT_KEYS
                .iter()
                .find(|(f, _)| *f == other)
                .map_or("", |(_, key)| key),
        }
    }

    /// The field a key names. `None` for an unknown key.
    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim();
        TEXT_KEYS
            .iter()
            .find(|(_, k)| *k == key)
            .map(|(f, _)| *f)
            .or_else(|| {
                ARTWORK_KEYS
                    .iter()
                    .find(|(_, k)| *k == key)
                    .map(|(kind, _)| Self::Artwork(*kind))
            })
    }

    /// Whether a scrape may write this field. Tags are never scraped: a provider's keywords are
    /// not tags, and the tags a title has (from its NFO or its edit page) stay as they are.
    /// The field still exists for editing and locking.
    pub fn is_scrapable(self) -> bool {
        !matches!(self, Self::Tags)
    }

    /// Whether items of `kind` have this field.
    pub fn applies_to(self, kind: ItemKind) -> bool {
        use MediaFileType as M;
        use MetaField as F;
        match kind {
            ItemKind::Movie => match self {
                F::Status => false,
                F::Artwork(a) => matches!(
                    a,
                    M::Poster
                        | M::Fanart
                        | M::Banner
                        | M::Clearart
                        | M::Disc
                        | M::Clearlogo
                        | M::Thumb
                        | M::Keyart
                        | M::Squareart
                        | M::Extrafanart
                        | M::Extrathumb
                ),
                _ => true,
            },
            ItemKind::Show => match self {
                F::SpokenLanguages | F::Collection => false,
                F::Artwork(a) => matches!(
                    a,
                    M::Poster
                        | M::Fanart
                        | M::Banner
                        | M::Clearart
                        | M::Disc
                        | M::Clearlogo
                        | M::Thumb
                        | M::Characterart
                        | M::Keyart
                        | M::Squareart
                        | M::Extrafanart
                ),
                _ => true,
            },
            ItemKind::Episode => matches!(
                self,
                F::Title
                    | F::OriginalTitle
                    | F::Plot
                    | F::ReleaseDate
                    | F::Rating
                    | F::Runtime
                    | F::Studio
                    | F::Tags
                    | F::Actors
                    | F::Crew
                    | F::Artwork(M::Thumb)
            ),
            ItemKind::Set => matches!(
                self,
                F::Title | F::Plot | F::Tags | F::Artwork(M::Poster | M::Fanart)
            ),
            ItemKind::Season => matches!(
                self,
                F::Artwork(M::SeasonPoster | M::SeasonBanner | M::SeasonThumb | M::SeasonFanart)
            ),
        }
    }
}

impl fmt::Display for MetaField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// A key that names no field.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown metadata field {0:?}")]
pub struct UnknownField(pub String);

impl FromStr for MetaField {
    type Err = UnknownField;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_key(s).ok_or_else(|| UnknownField(s.to_owned()))
    }
}

impl Serialize for MetaField {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

impl<'de> Deserialize<'de> for MetaField {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// A set of fields: the ones a scrape may write, or the ones locked on an item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldSet(BTreeSet<MetaField>);

impl FieldSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every field items of `kind` have.
    pub fn all_for(kind: ItemKind) -> Self {
        all_fields()
            .into_iter()
            .filter(|f| f.applies_to(kind))
            .collect()
    }

    /// Reads a comma-separated list of keys. Unknown keys are ignored, so a list written by a
    /// newer version still loads.
    pub fn from_keys(text: &str) -> Self {
        text.split(',').filter_map(MetaField::from_key).collect()
    }

    /// The comma-separated keys in list order.
    pub fn to_keys(&self) -> String {
        self.0.iter().map(|f| f.key()).collect::<Vec<_>>().join(",")
    }

    pub fn contains(&self, field: MetaField) -> bool {
        self.0.contains(&field)
    }

    pub fn insert(&mut self, field: MetaField) -> bool {
        self.0.insert(field)
    }

    pub fn remove(&mut self, field: MetaField) -> bool {
        self.0.remove(&field)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = MetaField> + '_ {
        self.0.iter().copied()
    }

    /// The keys, for writers that print them.
    pub fn keys(&self) -> Vec<String> {
        self.0.iter().map(|f| f.key().to_owned()).collect()
    }
}

impl FromIterator<MetaField> for FieldSet {
    fn from_iter<I: IntoIterator<Item = MetaField>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// What one scrape may write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrapeOptions {
    /// The fields the scrape may write.
    pub fields: FieldSet,
    /// Write only fields that are empty.
    pub do_not_overwrite: bool,
}

impl ScrapeOptions {
    /// Every field of `kind`, overwriting.
    pub fn all(kind: ItemKind) -> Self {
        Self {
            fields: FieldSet::all_for(kind),
            do_not_overwrite: false,
        }
    }

    /// Only the given fields, overwriting.
    pub fn only(fields: impl IntoIterator<Item = MetaField>) -> Self {
        Self {
            fields: fields.into_iter().collect(),
            do_not_overwrite: false,
        }
    }
}

/// Why a scrape changed nothing at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrapeBlock {
    /// The stored entity is locked.
    ItemLocked,
    /// The scraped item is of another kind than the stored one.
    KindMismatch,
}

/// What [`apply_scrape`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScrapeOutcome {
    pub blocked: Option<ScrapeBlock>,
    /// Fields whose stored value changed.
    pub written: Vec<MetaField>,
    /// Selected fields the provider had a value for but a field lock protected.
    pub locked: Vec<MetaField>,
    /// Selected fields kept because they were filled and *do not overwrite* was on.
    pub kept: Vec<MetaField>,
    /// Provider IDs that were added.
    pub ids_added: usize,
    /// Fields fetched again with the same value: the ratings, whose fetch time is renewed. The
    /// item is stored again, but nothing counts as changed.
    pub refreshed: Vec<MetaField>,
    /// The original language was set or corrected. It is not a field of the scrape dialog and
    /// not part of the NFO, so it does not count as a change; the item is stored again.
    pub original_language_set: bool,
}

impl ScrapeOutcome {
    /// Whether a value of the stored item changed.
    pub fn changed(&self) -> bool {
        !self.written.is_empty() || self.ids_added > 0
    }

    /// Whether the item must be stored: a value changed, or a rating was fetched again.
    pub fn touched(&self) -> bool {
        self.changed() || !self.refreshed.is_empty() || self.original_language_set
    }
}

// ---- field access -------------------------------------------------------------------------

/// The value of one field, in a form that can be compared and tested for emptiness.
#[derive(Debug, Clone, PartialEq)]
enum Slot {
    None,
    Text(String),
    Number(u64),
    Date(Option<CalendarDate>),
    Strings(Vec<String>),
    Genres(Vec<Genre>),
    Ratings(Vec<Rating>),
    Credits(Vec<Credit>),
    Trailers(Vec<Trailer>),
}

impl Slot {
    fn is_empty(&self) -> bool {
        match self {
            Self::None => true,
            Self::Text(t) => t.trim().is_empty(),
            Self::Number(n) => *n == 0,
            Self::Date(d) => d.is_none(),
            Self::Strings(v) => v.is_empty(),
            Self::Genres(v) => v.is_empty(),
            Self::Ratings(v) => v.is_empty(),
            Self::Credits(v) => v.is_empty(),
            Self::Trailers(v) => v.is_empty(),
        }
    }
}

fn is_cast(kind: CreditKind) -> bool {
    matches!(kind, CreditKind::Actor | CreditKind::Guest)
}

fn slot(item: &Item, field: MetaField) -> Slot {
    use MetaField as F;
    let c = &item.core;
    match field {
        F::Title => return Slot::Text(c.title.clone()),
        F::OriginalTitle => return Slot::Text(c.original_title.clone()),
        F::Plot => return Slot::Text(c.plot.clone()),
        F::Year => return Slot::Number(u64::from(c.year)),
        F::Studio => return Slot::Strings(c.production_companies.clone()),
        F::Tags => return Slot::Strings(c.tags.iter().map(str::to_owned).collect()),
        F::Rating => {
            // When a rating was fetched is not part of its value: the library stamps a rating
            // that arrives without a time, so the same rating fetched again is no change.
            return Slot::Ratings(
                c.ratings
                    .iter()
                    .filter(|r| r.id != rating_key::USER)
                    .map(|r| Rating {
                        fetched_at_ms: 0,
                        url: String::new(),
                        ..r.clone()
                    })
                    .collect(),
            );
        }
        F::Actors => {
            return Slot::Credits(
                c.credits
                    .iter()
                    .filter(|x| is_cast(x.kind))
                    .cloned()
                    .collect(),
            );
        }
        F::Crew => {
            return Slot::Credits(
                c.credits
                    .iter()
                    .filter(|x| !is_cast(x.kind))
                    .cloned()
                    .collect(),
            );
        }
        F::Collection => {
            return Slot::Text(c.ids.get(id_key::TMDB_SET).unwrap_or_default().to_owned());
        }
        F::Artwork(MediaFileType::Extrafanart) => {
            return match &item.detail {
                ItemDetail::Movie(m) => Slot::Strings(m.extra_fanarts.clone()),
                ItemDetail::Show(s) => Slot::Strings(s.extra_fanart_urls.clone()),
                _ => Slot::None,
            };
        }
        F::Artwork(MediaFileType::Extrathumb) => {
            return match &item.detail {
                ItemDetail::Movie(m) => Slot::Strings(m.extra_thumbs.clone()),
                _ => Slot::None,
            };
        }
        F::Artwork(kind) => {
            return Slot::Text(c.artwork_urls.get(&kind).cloned().unwrap_or_default());
        }
        _ => {}
    }
    match (&item.detail, field) {
        (ItemDetail::Movie(m), F::Tagline) => Slot::Text(m.tagline.clone()),
        (ItemDetail::Movie(m), F::ReleaseDate) => Slot::Date(m.release_date),
        (ItemDetail::Movie(m), F::Top250) => Slot::Number(u64::from(m.top250)),
        (ItemDetail::Movie(m), F::Runtime) => Slot::Number(u64::from(m.runtime_minutes)),
        (ItemDetail::Movie(m), F::Certification) => Slot::Text(m.certification.0.clone()),
        (ItemDetail::Movie(m), F::Genres) => Slot::Genres(m.genres.clone()),
        (ItemDetail::Movie(m), F::SpokenLanguages) => Slot::Strings(m.spoken_languages.clone()),
        (ItemDetail::Movie(m), F::Country) => Slot::Strings(m.countries.clone()),
        (ItemDetail::Movie(m), F::Trailer) => Slot::Trailers(m.trailers.clone()),
        (ItemDetail::Show(s), F::Tagline) => Slot::Text(s.tagline.clone()),
        (ItemDetail::Show(s), F::ReleaseDate) => Slot::Date(s.first_aired),
        (ItemDetail::Show(s), F::Status) => match s.status {
            AiredStatus::Unknown => Slot::None,
            known => Slot::Text(format!("{known:?}")),
        },
        (ItemDetail::Show(s), F::Top250) => Slot::Number(u64::from(s.top250)),
        (ItemDetail::Show(s), F::Runtime) => Slot::Number(u64::from(s.runtime_minutes)),
        (ItemDetail::Show(s), F::Certification) => Slot::Text(s.certification.0.clone()),
        (ItemDetail::Show(s), F::Genres) => Slot::Genres(s.genres.clone()),
        (ItemDetail::Show(s), F::Country) => Slot::Text(s.country.clone()),
        (ItemDetail::Show(s), F::Trailer) => Slot::Trailers(s.trailers.clone()),
        (ItemDetail::Episode(e), F::ReleaseDate) => Slot::Date(e.first_aired),
        (ItemDetail::Episode(e), F::Runtime) => Slot::Number(u64::from(e.runtime_minutes)),
        _ => Slot::None,
    }
}

/// Whether the field holds a value in the item.
fn has_value(item: &Item, field: MetaField) -> bool {
    !slot(item, field).is_empty()
}

/// Copies the value of `field` from `from` to `to`, exactly (an empty source empties the
/// target). Both items are of the same kind.
fn copy_field(to: &mut Item, from: &Item, field: MetaField) {
    use MetaField as F;
    match field {
        F::Title => to.core.title.clone_from(&from.core.title),
        F::OriginalTitle => to.core.original_title.clone_from(&from.core.original_title),
        F::Plot => to.core.plot.clone_from(&from.core.plot),
        F::Year => to.core.year = from.core.year,
        F::Studio => to
            .core
            .production_companies
            .clone_from(&from.core.production_companies),
        F::Tags => to.core.tags.clone_from(&from.core.tags),
        F::Rating => copy_scraped_ratings(to, from),
        F::Actors => copy_credits(to, from, true),
        F::Crew => copy_credits(to, from, false),
        F::Collection => {
            let value = from.core.ids.get(id_key::TMDB_SET).unwrap_or_default();
            to.core.ids.set(id_key::TMDB_SET, value);
        }
        F::Artwork(MediaFileType::Extrafanart) => match (&mut to.detail, &from.detail) {
            (ItemDetail::Movie(t), ItemDetail::Movie(f)) => {
                t.extra_fanarts.clone_from(&f.extra_fanarts);
            }
            (ItemDetail::Show(t), ItemDetail::Show(f)) => {
                t.extra_fanart_urls.clone_from(&f.extra_fanart_urls);
            }
            _ => {}
        },
        F::Artwork(MediaFileType::Extrathumb) => {
            if let (ItemDetail::Movie(t), ItemDetail::Movie(f)) = (&mut to.detail, &from.detail) {
                t.extra_thumbs.clone_from(&f.extra_thumbs);
            }
        }
        F::Artwork(kind) => match from.core.artwork_urls.get(&kind) {
            Some(url) => {
                to.core.artwork_urls.insert(kind, url.clone());
            }
            None => {
                to.core.artwork_urls.remove(&kind);
            }
        },
        _ => copy_detail_field(to, from, field),
    }
}

fn copy_credits(to: &mut Item, from: &Item, cast: bool) {
    let keep = to.core.credits.iter().filter(|x| is_cast(x.kind) != cast);
    let new = from.core.credits.iter().filter(|x| is_cast(x.kind) == cast);
    let merged: Vec<Credit> = if cast {
        new.chain(keep).cloned().collect()
    } else {
        keep.chain(new).cloned().collect()
    };
    to.core.credits = merged;
}

fn copy_detail_field(to: &mut Item, from: &Item, field: MetaField) {
    use MetaField as F;
    match (&mut to.detail, &from.detail) {
        (ItemDetail::Movie(t), ItemDetail::Movie(f)) => match field {
            F::Tagline => t.tagline.clone_from(&f.tagline),
            F::ReleaseDate => t.release_date = f.release_date,
            F::Top250 => t.top250 = f.top250,
            F::Runtime => t.runtime_minutes = f.runtime_minutes,
            F::Certification => t.certification.clone_from(&f.certification),
            F::Genres => t.genres.clone_from(&f.genres),
            F::SpokenLanguages => t.spoken_languages.clone_from(&f.spoken_languages),
            F::Country => t.countries.clone_from(&f.countries),
            F::Trailer => t.trailers.clone_from(&f.trailers),
            _ => {}
        },
        (ItemDetail::Show(t), ItemDetail::Show(f)) => match field {
            F::Tagline => t.tagline.clone_from(&f.tagline),
            F::ReleaseDate => t.first_aired = f.first_aired,
            F::Status => t.status = f.status,
            F::Top250 => t.top250 = f.top250,
            F::Runtime => t.runtime_minutes = f.runtime_minutes,
            F::Certification => t.certification.clone_from(&f.certification),
            F::Genres => t.genres.clone_from(&f.genres),
            F::Country => t.country.clone_from(&f.country),
            F::Trailer => t.trailers.clone_from(&f.trailers),
            _ => {}
        },
        (ItemDetail::Episode(t), ItemDetail::Episode(f)) => match field {
            F::ReleaseDate => t.first_aired = f.first_aired,
            F::Runtime => t.runtime_minutes = f.runtime_minutes,
            _ => {}
        },
        _ => {}
    }
}

/// Adds the scraped ratings to the stored ones, one by one, with the scale of each known source
/// normalised; the user's own rating stays.
fn merge_ratings(to: &mut Item, from: &Item) {
    for rating in from
        .core
        .ratings
        .iter()
        .filter(|r| r.id != rating_key::USER)
    {
        let mut rating = rating.clone().normalised();
        // A source that does not name the title's page keeps the address another one gave.
        if rating.url.is_empty()
            && let Some(old) = to.core.ratings.get(&rating.id)
        {
            rating.url.clone_from(&old.url);
        }
        to.core.ratings.insert(rating);
    }
}

/// Makes the source ratings of `to` equal to those of `from`. The user's own rating is a
/// different thing and keeps its value in `to`.
fn copy_scraped_ratings(to: &mut Item, from: &Item) {
    let user = to.core.ratings.get(rating_key::USER).cloned();
    let mut ratings: Ratings = from
        .core
        .ratings
        .iter()
        .filter(|r| !r.is_user())
        .cloned()
        .collect();
    if let Some(user) = user {
        ratings.insert(user);
    }
    to.core.ratings = ratings;
}

/// Whether the source ratings (everything but the user's own) of both items are equal.
fn same_source_ratings(a: &Item, b: &Item) -> bool {
    let strip = |i: &Item| -> Ratings {
        i.core
            .ratings
            .iter()
            .filter(|r| !r.is_user())
            .cloned()
            .collect()
    };
    strip(a) == strip(b)
}

// ---- the rules ---------------------------------------------------------------------------

/// Applies the `scraped` item to the `target` under the options, the entity lock and the field
/// `locks`. See the module documentation for the rules.
pub fn apply_scrape(
    target: &mut Item,
    scraped: &Item,
    options: &ScrapeOptions,
    locks: &FieldSet,
) -> ScrapeOutcome {
    let mut outcome = ScrapeOutcome::default();
    if target.kind() != scraped.kind() {
        outcome.blocked = Some(ScrapeBlock::KindMismatch);
        return outcome;
    }
    if target.core.locked {
        outcome.blocked = Some(ScrapeBlock::ItemLocked);
        return outcome;
    }
    let kind = target.kind();

    for field in options
        .fields
        .iter()
        .filter(|f| f.applies_to(kind) && f.is_scrapable())
    {
        if !has_value(scraped, field) {
            continue;
        }
        if locks.contains(field) {
            outcome.locked.push(field);
            continue;
        }
        if options.do_not_overwrite && has_value(target, field) {
            outcome.kept.push(field);
            continue;
        }
        let before = slot(target, field);
        let ratings_before = (field == MetaField::Rating).then(|| target.core.ratings.clone());
        if field == MetaField::Rating {
            merge_ratings(target, scraped);
        } else {
            copy_field(target, scraped, field);
        }
        if slot(target, field) != before {
            outcome.written.push(field);
        } else if ratings_before.is_some_and(|ratings| ratings != target.core.ratings) {
            // Only when they were fetched changed: stored again for the new date, no change.
            outcome.refreshed.push(field);
        }
    }

    // The original language comes with any scrape that knows it; it has no field of its own.
    if matches!(kind, ItemKind::Movie | ItemKind::Show)
        && !scraped.core.original_language.is_empty()
        && target.core.original_language != scraped.core.original_language
    {
        target
            .core
            .original_language
            .clone_from(&scraped.core.original_language);
        outcome.original_language_set = true;
    }

    // IDs are only added; the collection ID belongs to the Collection field.
    for (key, value) in scraped.core.ids.iter() {
        if key != id_key::TMDB_SET
            && !target.core.ids.contains(key)
            && target.core.ids.set(key, value)
        {
            outcome.ids_added += 1;
        }
    }
    outcome
}

/// Prepares `target` for a scrape of a *different* title (a change of match): the data of the old
/// title must not survive where the new one has none. For every selected, unlocked field that
/// `scraped` has no value for, the stored value is emptied, and the source ratings are always
/// emptied (the user's own rating stays), so the scrape and the rating fetch after it fill them
/// for the new title only. The title is never emptied. With *do not overwrite* nothing is
/// emptied. Returns the fields that were emptied.
pub fn clear_for_relink(
    target: &mut Item,
    scraped: &Item,
    options: &ScrapeOptions,
    locks: &FieldSet,
) -> Vec<MetaField> {
    let mut cleared = Vec::new();
    if options.do_not_overwrite || target.core.locked || target.kind() != scraped.kind() {
        return cleared;
    }
    let kind = target.kind();
    let blank = match kind {
        ItemKind::Show => Item::show(""),
        ItemKind::Episode => Item::episode(crate::ItemId(0), ""),
        ItemKind::Season => return cleared,
        ItemKind::Set => Item::set(""),
        ItemKind::Movie => Item::movie(""),
    };
    for field in options.fields.iter().filter(|f| f.applies_to(kind)) {
        if field == MetaField::Title || locks.contains(field) {
            continue;
        }
        if field != MetaField::Rating && has_value(scraped, field) {
            continue;
        }
        let before = slot(target, field);
        let ratings_before = target.core.ratings.clone();
        copy_field(target, &blank, field);
        if slot(target, field) != before || target.core.ratings != ratings_before {
            cleared.push(field);
        }
    }
    cleared
}

// ---- comparing for a preview ---------------------------------------------------------------

/// How the incoming value of a field compares with the stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldChange {
    /// Applying the scrape would change nothing: the values are equal, or the provider has no
    /// value (which never blanks a stored one).
    Same,
    /// The stored field is empty and the provider has a value.
    Added,
    /// The stored field has a value and applying the scrape would change it.
    Changed,
}

/// One field of the stored item next to the same field of an incoming item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldComparison {
    /// The stored value as plain text, `None` when the field is empty.
    pub current: Option<String>,
    /// The incoming value as plain text, `None` when the provider has none.
    pub incoming: Option<String>,
    pub change: FieldChange,
}

impl Slot {
    /// The value as plain text for a diff; `None` when empty.
    fn describe(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        Some(match self {
            Self::None => return None,
            Self::Text(t) => t.trim().to_owned(),
            Self::Number(n) => n.to_string(),
            Self::Date(d) => d.map(|d| d.to_string())?,
            Self::Strings(v) => v.join(", "),
            Self::Genres(v) => v
                .iter()
                .map(|g| g.name().to_owned())
                .collect::<Vec<_>>()
                .join(", "),
            Self::Ratings(v) => v
                .iter()
                .map(|r| format!("{:.1} {}", r.on_ten_scale(), r.id))
                .collect::<Vec<_>>()
                .join(", "),
            Self::Credits(v) => {
                const SHOWN: usize = 4;
                let names: Vec<&str> = v
                    .iter()
                    .take(SHOWN)
                    .map(|c| c.person.name.as_str())
                    .collect();
                let more = v.len().saturating_sub(SHOWN);
                if more > 0 {
                    format!("{} +{more} more", names.join(", "))
                } else {
                    names.join(", ")
                }
            }
            Self::Trailers(v) => match v.len() {
                1 => "1 trailer".to_owned(),
                n => format!("{n} trailers"),
            },
        })
    }
}

/// Compares `field` of the stored item with the incoming one the way [`apply_scrape`] would treat
/// it: a field the provider has no value for never counts as a change, and a rating the stored
/// item has already is no change either. Both items are of the same kind.
#[must_use]
pub fn compare_field(current: &Item, incoming: &Item, field: MetaField) -> FieldComparison {
    let before = slot(current, field);
    let wanted = slot(incoming, field);
    let incoming_text = wanted.describe();
    let current_text = before.describe();
    let change = if wanted.is_empty() {
        FieldChange::Same
    } else {
        // Try the write on a copy: what changes there changes for real.
        let mut probe = current.clone();
        if field == MetaField::Rating {
            merge_ratings(&mut probe, incoming);
        } else {
            copy_field(&mut probe, incoming, field);
        }
        if slot(&probe, field) == before {
            FieldChange::Same
        } else if before.is_empty() {
            FieldChange::Added
        } else {
            FieldChange::Changed
        }
    };
    FieldComparison {
        current: current_text,
        incoming: incoming_text,
        change,
    }
}

/// Puts every locked field of `updated` back to its value in `original`, for writers that are
/// not scrapes. Returns the fields that were put back. Does nothing for another kind.
pub fn revert_locked(updated: &mut Item, original: &Item, locks: &FieldSet) -> Vec<MetaField> {
    let mut reverted = Vec::new();
    if updated.kind() != original.kind() {
        return reverted;
    }
    for field in locks.iter() {
        if slot(updated, field) == slot(original, field)
            && (field != MetaField::Rating || same_source_ratings(updated, original))
        {
            continue;
        }
        copy_field(updated, original, field);
        reverted.push(field);
    }
    reverted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored() -> Item {
        let mut item = Item::movie("My title");
        item.core.plot = "My own plot".into();
        item.core.year = 1999;
        item.core.ratings.insert(Rating::new("user", 9.0, 0, 10));
        item.core.ratings.insert(Rating::new("imdb", 7.0, 10, 10));
        item.core.ids.set("imdb", "tt1");
        item.core
            .artwork_urls
            .insert(MediaFileType::Fanart, "https://old/fanart".into());
        if let ItemDetail::Movie(m) = &mut item.detail {
            m.genres = vec![Genre::Drama];
            m.runtime_minutes = 100;
        }
        item
    }

    fn scraped() -> Item {
        let mut item = Item::movie("Scraped title");
        item.core.plot = "Scraped plot".into();
        item.core.year = 2001;
        item.core.ratings.insert(Rating::new("imdb", 8.0, 99, 10));
        item.core.ratings.insert(Rating::new("tmdb", 7.5, 5, 10));
        item.core.ids.set("imdb", "tt-other");
        item.core.ids.set("tmdb", "42");
        item.core.ids.set(id_key::TMDB_SET, "7");
        item.core
            .artwork_urls
            .insert(MediaFileType::Fanart, "https://new/fanart".into());
        item.core
            .artwork_urls
            .insert(MediaFileType::Poster, "https://new/poster".into());
        item.core
            .credits
            .push(Credit::new(CreditKind::Actor, "A. Actor", "Hero"));
        item.core
            .credits
            .push(Credit::new(CreditKind::Director, "D. Director", ""));
        if let ItemDetail::Movie(m) = &mut item.detail {
            m.genres = vec![Genre::Comedy];
            m.runtime_minutes = 120;
        }
        item
    }

    #[test]
    fn a_change_of_match_empties_what_the_new_title_lacks_and_every_source_rating() {
        let mut target = stored();
        target
            .core
            .ratings
            .insert(Rating::new("tomatometerallcritics", 84.0, 0, 100));
        target
            .core
            .artwork_urls
            .insert(MediaFileType::Clearlogo, "https://old/logo".into());
        target.core.ids.set(id_key::TMDB_SET, "1048282");
        let mut scraped = scraped();
        scraped.core.ids.remove(id_key::TMDB_SET);
        let options = ScrapeOptions::all(ItemKind::Movie);

        let cleared = clear_for_relink(&mut target, &scraped, &options, &FieldSet::default());

        // The old film's ratings, logo and collection are gone; the user's rating stays.
        assert!(cleared.contains(&MetaField::Rating));
        assert_eq!(target.core.ratings.get("tomatometerallcritics"), None);
        assert_eq!(target.core.ratings.get("imdb"), None);
        assert_eq!(target.core.ratings.user_rating(), Some(9.0));
        assert_eq!(
            target.core.artwork_urls.get(&MediaFileType::Clearlogo),
            None
        );
        assert_eq!(target.core.ids.get(id_key::TMDB_SET), None);
        // What the new title has is left for the scrape to write, and the title is never emptied.
        assert_eq!(target.core.plot, "My own plot");
        assert_eq!(target.core.title, "My title");

        // A locked field and "do not overwrite" keep their values.
        let mut kept = stored();
        kept.core
            .ratings
            .insert(Rating::new("metacritic", 76.0, 0, 100));
        let locks: FieldSet = [MetaField::Rating].into_iter().collect();
        assert!(
            !clear_for_relink(&mut kept, &scraped, &options, &locks).contains(&MetaField::Rating)
        );
        assert!(kept.core.ratings.get("metacritic").is_some());
        let only_empty = ScrapeOptions {
            do_not_overwrite: true,
            ..options
        };
        assert!(
            clear_for_relink(&mut kept, &scraped, &only_empty, &FieldSet::default()).is_empty()
        );
    }

    #[test]
    fn keys_round_trip_and_unknown_keys_are_ignored() {
        for field in all_fields() {
            assert_eq!(MetaField::from_key(field.key()), Some(field), "{field}");
        }
        let set = FieldSet::from_keys("plot, fanart,nonsense,,genres");
        assert_eq!(set.to_keys(), "plot,genres,fanart");
        assert_eq!(
            serde_json::to_string(&MetaField::Artwork(MediaFileType::Fanart)).unwrap(),
            "\"fanart\""
        );
        assert!(serde_json::from_str::<MetaField>("\"nope\"").is_err());
    }

    #[test]
    fn every_kind_has_its_fields() {
        let movie = FieldSet::all_for(ItemKind::Movie);
        assert!(movie.contains(MetaField::Collection) && !movie.contains(MetaField::Status));
        let show = FieldSet::all_for(ItemKind::Show);
        assert!(show.contains(MetaField::Status) && !show.contains(MetaField::SpokenLanguages));
        let episode = FieldSet::all_for(ItemKind::Episode);
        assert!(episode.contains(MetaField::Artwork(MediaFileType::Thumb)));
        assert!(!episode.contains(MetaField::Genres));
    }

    #[test]
    fn only_the_selected_fields_change() {
        let mut target = stored();
        let options =
            ScrapeOptions::only([MetaField::Rating, MetaField::Artwork(MediaFileType::Fanart)]);
        let outcome = apply_scrape(&mut target, &scraped(), &options, &FieldSet::new());
        assert_eq!(
            outcome.written,
            [MetaField::Rating, MetaField::Artwork(MediaFileType::Fanart)]
        );
        assert_eq!(target.core.title, "My title");
        assert_eq!(target.core.plot, "My own plot");
        assert_eq!(target.core.year, 1999);
        assert_eq!(target.core.ratings.get("imdb").map(|r| r.votes), Some(99));
        assert!(target.core.ratings.get("tmdb").is_some());
        assert_eq!(
            target.core.ratings.get("user").map(|r| r.value),
            Some(9.0),
            "the user's rating stays"
        );
        assert_eq!(
            target
                .core
                .artwork_urls
                .get(&MediaFileType::Fanart)
                .map(String::as_str),
            Some("https://new/fanart")
        );
        assert!(
            !target
                .core
                .artwork_urls
                .contains_key(&MediaFileType::Poster)
        );
        let ItemDetail::Movie(m) = &target.detail else {
            panic!("not a movie")
        };
        assert_eq!(m.genres, [Genre::Drama]);
        assert_eq!(m.runtime_minutes, 100);
    }

    #[test]
    fn a_locked_plot_survives_a_rescrape_and_everything_else_updates() {
        let mut target = stored();
        let locks: FieldSet = [MetaField::Plot].into_iter().collect();
        let outcome = apply_scrape(
            &mut target,
            &scraped(),
            &ScrapeOptions::all(ItemKind::Movie),
            &locks,
        );
        assert_eq!(target.core.plot, "My own plot");
        assert_eq!(outcome.locked, [MetaField::Plot]);
        assert!(!outcome.written.contains(&MetaField::Plot));
        assert_eq!(target.core.title, "Scraped title");
        assert_eq!(target.core.year, 2001);
        let ItemDetail::Movie(m) = &target.detail else {
            panic!("not a movie")
        };
        assert_eq!(m.genres, [Genre::Comedy]);
        assert_eq!(m.runtime_minutes, 120);
    }

    #[test]
    fn a_locked_genre_field_keeps_the_users_genres() {
        let mut target = stored();
        let locks: FieldSet = [MetaField::Genres].into_iter().collect();
        apply_scrape(
            &mut target,
            &scraped(),
            &ScrapeOptions::all(ItemKind::Movie),
            &locks,
        );
        let ItemDetail::Movie(m) = &target.detail else {
            panic!("not a movie")
        };
        assert_eq!(m.genres, [Genre::Drama]);
    }

    #[test]
    fn a_locked_entity_is_not_modified() {
        let mut target = stored();
        target.core.locked = true;
        let before = target.clone();
        let outcome = apply_scrape(
            &mut target,
            &scraped(),
            &ScrapeOptions::all(ItemKind::Movie),
            &FieldSet::new(),
        );
        assert_eq!(outcome.blocked, Some(ScrapeBlock::ItemLocked));
        assert!(!outcome.changed());
        assert_eq!(target, before);
    }

    #[test]
    fn do_not_overwrite_writes_only_empty_fields() {
        let mut target = stored();
        let options = ScrapeOptions {
            do_not_overwrite: true,
            ..ScrapeOptions::all(ItemKind::Movie)
        };
        let outcome = apply_scrape(&mut target, &scraped(), &options, &FieldSet::new());
        assert_eq!(target.core.title, "My title");
        assert_eq!(target.core.plot, "My own plot");
        assert_eq!(target.core.year, 1999);
        assert!(outcome.kept.contains(&MetaField::Plot));
        assert!(outcome.kept.contains(&MetaField::Genres));
        // Empty fields are filled.
        assert_eq!(
            target
                .core
                .artwork_urls
                .get(&MediaFileType::Poster)
                .map(String::as_str),
            Some("https://new/poster")
        );
        assert_eq!(target.core.credits.len(), 2);
        assert!(outcome.written.contains(&MetaField::Actors));
        assert!(outcome.written.contains(&MetaField::Crew));
        assert_eq!(
            target
                .core
                .artwork_urls
                .get(&MediaFileType::Fanart)
                .map(String::as_str),
            Some("https://old/fanart")
        );
    }

    #[test]
    fn a_value_the_provider_lacks_never_blanks_a_stored_one() {
        let mut target = stored();
        let mut sparse = Item::movie("");
        sparse.core.year = 0;
        let outcome = apply_scrape(
            &mut target,
            &sparse,
            &ScrapeOptions::all(ItemKind::Movie),
            &FieldSet::new(),
        );
        assert!(!outcome.changed());
        assert_eq!(target, stored());
    }

    #[test]
    fn ids_are_added_but_never_replaced_and_the_collection_is_a_field() {
        let mut target = stored();
        let outcome = apply_scrape(
            &mut target,
            &scraped(),
            &ScrapeOptions::only([MetaField::Title]),
            &FieldSet::new(),
        );
        assert_eq!(target.core.ids.get("imdb"), Some("tt1"));
        assert_eq!(target.core.ids.get("tmdb"), Some("42"));
        assert_eq!(outcome.ids_added, 1);
        assert_eq!(target.core.ids.get(id_key::TMDB_SET), None);

        let outcome = apply_scrape(
            &mut target,
            &scraped(),
            &ScrapeOptions::only([MetaField::Collection]),
            &FieldSet::new(),
        );
        assert_eq!(target.core.ids.get(id_key::TMDB_SET), Some("7"));
        assert_eq!(outcome.written, [MetaField::Collection]);
    }

    #[test]
    fn cast_and_crew_are_separate_fields() {
        let mut target = stored();
        target
            .core
            .credits
            .push(Credit::new(CreditKind::Actor, "Old Actor", "Old"));
        target
            .core
            .credits
            .push(Credit::new(CreditKind::Writer, "Old Writer", ""));
        apply_scrape(
            &mut target,
            &scraped(),
            &ScrapeOptions::only([MetaField::Crew]),
            &FieldSet::new(),
        );
        let names: Vec<&str> = target
            .core
            .credits
            .iter()
            .map(|c| c.person.name.as_str())
            .collect();
        assert_eq!(names, ["Old Actor", "D. Director"]);
    }

    #[test]
    fn a_show_status_is_a_field_and_unknown_is_empty() {
        let mut target = Item::show("S");
        let mut scraped = Item::show("S");
        if let ItemDetail::Show(s) = &mut scraped.detail {
            s.status = AiredStatus::Ended;
        }
        let options = ScrapeOptions::only([MetaField::Status]);
        let outcome = apply_scrape(&mut target, &scraped, &options, &FieldSet::new());
        assert_eq!(outcome.written, [MetaField::Status]);
        assert!(matches!(&target.detail, ItemDetail::Show(s) if s.status == AiredStatus::Ended));
        // Unknown from the provider does not blank it.
        let blank = Item::show("S");
        let outcome = apply_scrape(&mut target, &blank, &options, &FieldSet::new());
        assert!(!outcome.changed());
    }

    #[test]
    fn another_kind_is_refused() {
        let mut target = stored();
        let outcome = apply_scrape(
            &mut target,
            &Item::show("S"),
            &ScrapeOptions::all(ItemKind::Movie),
            &FieldSet::new(),
        );
        assert_eq!(outcome.blocked, Some(ScrapeBlock::KindMismatch));
    }

    #[test]
    fn revert_locked_puts_locked_fields_back() {
        let original = stored();
        let mut updated = original.clone();
        updated.core.plot = "From the NFO".into();
        updated.core.title = "NFO title".into();
        updated.core.ratings.insert(Rating::new("imdb", 1.0, 1, 10));
        let locks: FieldSet = [MetaField::Plot, MetaField::Rating, MetaField::Genres]
            .into_iter()
            .collect();
        let reverted = revert_locked(&mut updated, &original, &locks);
        assert_eq!(reverted, [MetaField::Plot, MetaField::Rating]);
        assert_eq!(updated.core.plot, "My own plot");
        assert_eq!(updated.core.title, "NFO title");
        assert_eq!(updated.core.ratings, original.core.ratings);
        // Nothing to revert when nothing changed.
        let mut same = original.clone();
        assert!(revert_locked(&mut same, &original, &locks).is_empty());
    }

    #[test]
    fn the_user_rating_survives_every_rating_write() {
        // a scrape that carries a user rating of its own
        let mut incoming = scraped();
        incoming
            .core
            .ratings
            .insert(Rating::new("user", 1.0, 0, 10));
        for do_not_overwrite in [false, true] {
            let mut target = stored();
            let mut options = ScrapeOptions::only([MetaField::Rating]);
            options.do_not_overwrite = do_not_overwrite;
            apply_scrape(&mut target, &incoming, &options, &FieldSet::new());
            assert_eq!(target.core.ratings.user_rating(), Some(9.0));
        }
        // a locked rating field keeps everything, and so does the entity-wide select-all
        let mut target = stored();
        let locks: FieldSet = [MetaField::Rating].into_iter().collect();
        apply_scrape(
            &mut target,
            &incoming,
            &ScrapeOptions::all(ItemKind::Movie),
            &locks,
        );
        assert_eq!(target.core.ratings.user_rating(), Some(9.0));
        assert_eq!(target.core.ratings.get("imdb").map(|r| r.votes), Some(10));
        let mut target = stored();
        apply_scrape(
            &mut target,
            &incoming,
            &ScrapeOptions::all(ItemKind::Movie),
            &FieldSet::new(),
        );
        assert_eq!(target.core.ratings.user_rating(), Some(9.0));
        // a scrape into an item that has no user rating does not create one
        let mut fresh = Item::movie("Fresh");
        apply_scrape(
            &mut fresh,
            &incoming,
            &ScrapeOptions::all(ItemKind::Movie),
            &FieldSet::new(),
        );
        assert_eq!(fresh.core.ratings.user_rating(), None);
        assert!(fresh.core.ratings.get("imdb").is_some());
    }

    #[test]
    fn reverting_a_locked_rating_field_keeps_the_user_rating() {
        let original = stored();
        let mut updated = original.clone();
        updated.core.ratings.insert(Rating::new("imdb", 1.0, 1, 10));
        updated.core.ratings.set_user_rating(4.5).unwrap();
        let locks: FieldSet = [MetaField::Rating].into_iter().collect();
        assert_eq!(
            revert_locked(&mut updated, &original, &locks),
            [MetaField::Rating]
        );
        assert_eq!(updated.core.ratings.get("imdb").map(|r| r.votes), Some(10));
        assert_eq!(updated.core.ratings.user_rating(), Some(4.5));

        // changing only the user rating is not a change to the locked field
        let mut only_user = original.clone();
        only_user.core.ratings.set_user_rating(2.0).unwrap();
        assert!(revert_locked(&mut only_user, &original, &locks).is_empty());
        assert_eq!(only_user.core.ratings.user_rating(), Some(2.0));
    }

    #[test]
    fn scraped_ratings_get_their_known_scale() {
        let mut target = stored();
        let mut incoming = scraped();
        incoming
            .core
            .ratings
            .insert(Rating::new("letterboxd", 4.4, 100, 10));
        incoming
            .core
            .ratings
            .insert(Rating::new("themoviedb", 6.0, 1, 10));
        apply_scrape(
            &mut target,
            &incoming,
            &ScrapeOptions::only([MetaField::Rating]),
            &FieldSet::new(),
        );
        assert_eq!(target.core.ratings.get("letterboxd").unwrap().max_value, 5);
        assert!(target.core.ratings.get("themoviedb").is_none());
    }

    #[test]
    fn the_same_rating_fetched_again_is_no_change() {
        let mut target = stored();
        if let Some(stamped) = target.core.ratings.get("imdb").cloned() {
            target
                .core
                .ratings
                .insert(stamped.fetched_at(1_700_000_000_000));
        }
        let mut again = Item::movie("My title");
        again.core.ratings.insert(Rating::new("imdb", 7.0, 10, 10));
        let outcome = apply_scrape(
            &mut target,
            &again,
            &ScrapeOptions::only([MetaField::Rating]),
            &FieldSet::new(),
        );
        assert!(outcome.written.is_empty(), "{outcome:?}");
        assert!(!outcome.changed());
        // It is stored again, so the library knows it was fetched now.
        assert_eq!(outcome.refreshed, [MetaField::Rating]);
        assert!(outcome.touched());
    }

    #[test]
    fn a_comparison_says_what_applying_would_do_to_each_field() {
        let (stored, scraped) = (stored(), scraped());
        let compare = |field| compare_field(&stored, &scraped, field);

        // Both have a plot: it would change.
        let plot = compare(MetaField::Plot);
        assert_eq!(plot.change, FieldChange::Changed);
        assert_eq!(plot.current.as_deref(), Some("My own plot"));
        assert_eq!(plot.incoming.as_deref(), Some("Scraped plot"));
        // The stored item has no tagline and the provider none either: nothing to show.
        let tagline = compare(MetaField::Tagline);
        assert_eq!(tagline.change, FieldChange::Same);
        assert_eq!((tagline.current, tagline.incoming), (None, None));
        // Fields only the provider has are additions.
        assert_eq!(compare(MetaField::Actors).change, FieldChange::Added);
        assert_eq!(
            compare(MetaField::Artwork(MediaFileType::Poster)).change,
            FieldChange::Added
        );
        assert_eq!(
            compare(MetaField::Crew).incoming.as_deref(),
            Some("D. Director")
        );
        // The same artwork type with another address changes.
        assert_eq!(
            compare(MetaField::Artwork(MediaFileType::Fanart)).change,
            FieldChange::Changed
        );
        assert_eq!(
            compare(MetaField::Genres).incoming.as_deref(),
            Some("COMEDY")
        );
        assert_eq!(compare(MetaField::Runtime).current.as_deref(), Some("100"));
    }

    #[test]
    fn a_value_the_provider_lacks_and_a_rating_already_held_are_no_difference() {
        let stored = stored();
        // The provider has no plot: the stored one stays, so it is not a difference.
        let mut bare = Item::movie("Bare");
        bare.core.year = 1999;
        assert_eq!(
            compare_field(&stored, &bare, MetaField::Plot).change,
            FieldChange::Same
        );
        assert_eq!(
            compare_field(&stored, &bare, MetaField::Year).change,
            FieldChange::Same
        );
        // The same IMDb rating again is no change; a new source is an addition to the ratings.
        let mut again = Item::movie("Again");
        again.core.ratings.insert(Rating::new("imdb", 7.0, 10, 10));
        assert_eq!(
            compare_field(&stored, &again, MetaField::Rating).change,
            FieldChange::Same
        );
        again.core.ratings.insert(Rating::new("tmdb", 7.5, 5, 10));
        assert_eq!(
            compare_field(&stored, &again, MetaField::Rating).change,
            FieldChange::Changed
        );
    }
}
