//! The editable fields of an item, as plain values.
//!
//! [`read_fields`] turns an item into [`FieldEntry`] values and [`apply_change`] writes one value
//! back, checking it first. Both work on keys, so the detail pages of every kind share one editor.
//! Which key is locked by which scrape field is [`lock_key`].
//!
//! Direct edits are never blocked by a lock: a lock protects a field from machines, not from its
//! owner.

use cv_domain::{
    AiredStatus, CalendarDate, Certification, Credit, CreditKind, Edition, EpisodeGroup,
    EpisodeNumber, EpisodeNumbers, EpisodeOrdering, Genre, Item, ItemDetail, ItemKind,
    MediaFileType, MediaSource, MetaField, ProviderIds, Rating, Ratings, SeasonText, Tags, Trailer,
    canonical_rating_id, rating_key,
};

/// A key value pair: a provider ID, or an artwork URL by type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub key: String,
    pub value: String,
}

/// One credit in an editable list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonValue {
    /// `actor`, `guest`, `director`, `writer`, `producer`, `composer`, `editor`, `camera`, `other`.
    pub kind: String,
    pub name: String,
    pub role: String,
}

/// One trailer in an editable list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrailerValue {
    pub name: String,
    pub url: String,
    pub quality: String,
    pub language: String,
    pub in_nfo: bool,
}

/// An episode's season and episode number in one ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberValue {
    /// `aired`, `dvd`, `absolute`, `alternate` or `display`.
    pub ordering: String,
    /// The provider's name for a custom ordering; empty for the standard ones.
    pub name: String,
    pub season: u32,
    pub episode: u32,
}

/// One source rating in an editable list. The user's own rating is the `user_rating` field.
#[derive(Debug, Clone, PartialEq)]
pub struct RatingValue {
    /// The rating source: `imdb`, `tmdb`, `metacritic` and so on.
    pub source: String,
    /// The rating on the scale of the source.
    pub value: f64,
    /// The top of the scale: 10, 5, 4 or 100.
    pub max: u32,
    pub votes: u32,
}

/// The value of one field.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Text(String),
    /// A number; `None` clears the field.
    Number(Option<f64>),
    Flag(bool),
    /// `YYYY-MM-DD`; `None` clears the field.
    Date(Option<String>),
    List(Vec<String>),
    Pairs(Vec<Pair>),
    People(Vec<PersonValue>),
    Trailers(Vec<TrailerValue>),
    Numbers(Vec<NumberValue>),
    Ratings(Vec<RatingValue>),
}

/// A field with its current value and the scrape field that locks it, when it has one.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldEntry {
    pub key: String,
    pub value: FieldValue,
    /// The lock key (`MetaField::key`) that protects the field from scraping.
    pub lock: Option<String>,
}

/// Why a value was refused. `key` names the field, `message` is ready to show.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{key}: {message}")]
pub struct FieldError {
    pub key: String,
    pub message: String,
}

impl FieldError {
    /// A refusal of `key`. The message is shown on its own next to the field and after
    /// "Nothing was changed.", so it is written as a sentence: capital first letter.
    #[must_use]
    pub fn new(key: &str, message: impl Into<String>) -> Self {
        Self {
            key: key.to_owned(),
            message: sentence(&message.into()),
        }
    }
}

/// The text with its first letter in capitals.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn refuse(key: &str, message: impl Into<String>) -> FieldError {
    FieldError::new(key, message)
}

/// The years the editors accept; empty (0) means unknown.
pub const YEAR_MIN: u16 = 1800;
pub const YEAR_MAX: u16 = 2200;
const YEAR_RULE: &str = "enter a year from 1800 to 2200, or leave it empty";

/// Prefix of the artwork URL fields: `artwork.poster`, `artwork.fanart`, and so on.
pub const ARTWORK_PREFIX: &str = "artwork.";
/// Prefix of the season name fields of a show: `season_name.2`.
pub const SEASON_NAME_PREFIX: &str = "season_name.";
/// Prefix of the season overview fields of a show: `season_overview.2`.
pub const SEASON_OVERVIEW_PREFIX: &str = "season_overview.";

const MOVIE_KEYS: &[&str] = &[
    "title",
    "original_title",
    "sort_title",
    "tagline",
    "plot",
    "outline",
    "year",
    "release_date",
    "runtime",
    "certification",
    "genres",
    "studios",
    "countries",
    "spoken_languages",
    "edition",
    "media_source",
    "video_in_3d",
    "tags",
    "note",
    "user_rating",
    "ratings",
    "play_count",
    "last_watched",
    "top250",
    "ids",
    "cast",
    "crew",
    "trailers",
];
const SET_KEYS: &[&str] = &["title", "sort_title", "plot", "tags"];
const SHOW_KEYS: &[&str] = &[
    "title",
    "original_title",
    "sort_title",
    "tagline",
    "plot",
    "outline",
    "year",
    "first_aired",
    "status",
    "runtime",
    "certification",
    "genres",
    "studios",
    "country",
    "tags",
    "note",
    "user_rating",
    "ratings",
    "top250",
    "ids",
    "cast",
    "crew",
    "trailers",
];
const EPISODE_KEYS: &[&str] = &[
    "title",
    "original_title",
    "plot",
    "first_aired",
    "runtime",
    "edition",
    "media_source",
    "studios",
    "tags",
    "note",
    "user_rating",
    "ratings",
    "play_count",
    "last_watched",
    "ids",
    "numbers",
    "cast",
    "crew",
];

/// The artwork types with a URL field, in display order.
const ARTWORK_TYPES: &[MediaFileType] = &[
    MediaFileType::Poster,
    MediaFileType::Fanart,
    MediaFileType::Banner,
    MediaFileType::Clearlogo,
    MediaFileType::Clearart,
    MediaFileType::Disc,
    MediaFileType::Thumb,
    MediaFileType::Keyart,
    MediaFileType::Squareart,
    MediaFileType::Characterart,
];

fn plain_keys(kind: ItemKind) -> &'static [&'static str] {
    match kind {
        ItemKind::Movie => MOVIE_KEYS,
        ItemKind::Set => SET_KEYS,
        ItemKind::Show => SHOW_KEYS,
        ItemKind::Episode => EPISODE_KEYS,
        ItemKind::Season => &[],
    }
}

fn artwork_types(kind: ItemKind) -> impl Iterator<Item = MediaFileType> {
    ARTWORK_TYPES
        .iter()
        .copied()
        .filter(move |t| MetaField::Artwork(*t).applies_to(kind))
}

/// The scrape field that locks `key` on an item of `kind`. `None` for a field no scrape writes
/// (the sort title, edition, note, IDs and the like), which is protected by that fact alone.
pub fn lock_key(kind: ItemKind, key: &str) -> Option<&'static str> {
    let field = match key {
        "title" => MetaField::Title,
        "original_title" => MetaField::OriginalTitle,
        "tagline" => MetaField::Tagline,
        "plot" => MetaField::Plot,
        "year" => MetaField::Year,
        "release_date" | "first_aired" => MetaField::ReleaseDate,
        "status" => MetaField::Status,
        "top250" => MetaField::Top250,
        "ratings" => MetaField::Rating,
        "runtime" => MetaField::Runtime,
        "certification" => MetaField::Certification,
        "genres" => MetaField::Genres,
        "spoken_languages" => MetaField::SpokenLanguages,
        "countries" | "country" => MetaField::Country,
        "studios" => MetaField::Studio,
        "tags" => MetaField::Tags,
        "cast" => MetaField::Actors,
        "crew" => MetaField::Crew,
        "trailers" => MetaField::Trailer,
        other => {
            let name = other.strip_prefix(ARTWORK_PREFIX)?;
            MetaField::from_key(name).filter(|f| matches!(f, MetaField::Artwork(_)))?
        }
    };
    field.applies_to(kind).then(|| field.key())
}

/// Every field key an item of `kind` offers, artwork included.
pub fn field_keys(item: &Item) -> Vec<String> {
    let kind = item.kind();
    let mut keys: Vec<String> = plain_keys(kind).iter().map(|k| (*k).to_owned()).collect();
    keys.extend(
        artwork_types(kind).map(|t| format!("{ARTWORK_PREFIX}{}", MetaField::Artwork(t).key())),
    );
    if let ItemDetail::Show(show) = &item.detail {
        for text in show
            .season_texts
            .iter()
            .filter(|t| t.group == show.episode_group)
        {
            keys.push(format!("{SEASON_NAME_PREFIX}{}", text.season));
            keys.push(format!("{SEASON_OVERVIEW_PREFIX}{}", text.season));
        }
    }
    keys
}

fn kind_name(kind: CreditKind) -> &'static str {
    match kind {
        CreditKind::Actor => "actor",
        CreditKind::Director => "director",
        CreditKind::Writer => "writer",
        CreditKind::Producer => "producer",
        CreditKind::Guest => "guest",
        CreditKind::Composer => "composer",
        CreditKind::Editor => "editor",
        CreditKind::Camera => "camera",
        CreditKind::Other => "other",
    }
}

fn parse_kind(text: &str) -> CreditKind {
    match text.trim().to_ascii_lowercase().as_str() {
        "actor" => CreditKind::Actor,
        "director" => CreditKind::Director,
        "writer" => CreditKind::Writer,
        "producer" => CreditKind::Producer,
        "guest" => CreditKind::Guest,
        "composer" => CreditKind::Composer,
        "editor" => CreditKind::Editor,
        "camera" => CreditKind::Camera,
        _ => CreditKind::Other,
    }
}

fn is_cast(kind: CreditKind) -> bool {
    matches!(kind, CreditKind::Actor | CreditKind::Guest)
}

fn ordering_name(ordering: EpisodeOrdering) -> &'static str {
    match ordering {
        EpisodeOrdering::Aired => "aired",
        EpisodeOrdering::Dvd => "dvd",
        EpisodeOrdering::Absolute => "absolute",
        EpisodeOrdering::Alternate => "alternate",
        EpisodeOrdering::Display => "display",
    }
}

fn parse_ordering(text: &str) -> Option<EpisodeOrdering> {
    Some(match text.trim().to_ascii_lowercase().as_str() {
        "aired" => EpisodeOrdering::Aired,
        "dvd" => EpisodeOrdering::Dvd,
        "absolute" => EpisodeOrdering::Absolute,
        "alternate" => EpisodeOrdering::Alternate,
        "display" => EpisodeOrdering::Display,
        _ => return None,
    })
}

fn status_name(status: AiredStatus) -> &'static str {
    match status {
        AiredStatus::Unknown => "unknown",
        AiredStatus::Continuing => "continuing",
        AiredStatus::Ended => "ended",
        AiredStatus::Canceled => "canceled",
    }
}

fn date_text(date: Option<CalendarDate>) -> FieldValue {
    FieldValue::Date(date.map(|d| d.to_string()))
}

/// The local day an instant falls on, `yyyy-MM-dd`.
fn local_day(ms: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms).map(|time| {
        time.with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string()
    })
}

/// When the title was last watched, as the local day; the time of day is not edited.
fn watched_day(ms: Option<i64>) -> FieldValue {
    FieldValue::Date(ms.filter(|ms| *ms > 0).and_then(local_day))
}

/// The last-watched instant for an edited day. The stored instant stays when it falls on that
/// day already, so saving the form unchanged (or undoing) keeps the time of day; a new day is
/// taken at local noon, which exists on every day of the year.
fn watched_instant(
    key: &str,
    value: &FieldValue,
    current: Option<i64>,
) -> Result<Option<i64>, FieldError> {
    let Some(day) = want_date(key, value)? else {
        return Ok(None);
    };
    let text = day.to_string();
    if let Some(ms) = current.filter(|ms| local_day(*ms).as_deref() == Some(text.as_str())) {
        return Ok(Some(ms));
    }
    let noon =
        chrono::NaiveDate::from_ymd_opt(day.year(), u32::from(day.month()), u32::from(day.day()))
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .and_then(|time| time.and_local_timezone(chrono::Local).earliest())
            .ok_or_else(|| refuse(key, "enter a real day, such as 2024-03-01"))?;
    Ok(Some(noon.timestamp_millis()))
}

/// A zero stands for "not set" in the library, so it reads as an empty number.
fn count(value: u32) -> FieldValue {
    FieldValue::Number((value > 0).then(|| f64::from(value)))
}

fn genre_names(genres: &[Genre]) -> FieldValue {
    FieldValue::List(genres.iter().map(|g| g.name().to_owned()).collect())
}

fn edition_text(edition: &Edition) -> FieldValue {
    FieldValue::Text(if *edition == Edition::None {
        String::new()
    } else {
        edition.name().to_owned()
    })
}

fn source_text(source: &MediaSource) -> FieldValue {
    FieldValue::Text(if *source == MediaSource::Unknown {
        String::new()
    } else {
        source.name().to_owned()
    })
}

fn credits_value(item: &Item, cast: bool) -> FieldValue {
    FieldValue::People(
        item.core
            .credits
            .iter()
            .filter(|c| is_cast(c.kind) == cast)
            .map(|c| PersonValue {
                kind: kind_name(c.kind).to_owned(),
                name: c.person.name.clone(),
                role: c.role.clone(),
            })
            .collect(),
    )
}

fn trailers_value(trailers: &[Trailer]) -> FieldValue {
    FieldValue::Trailers(
        trailers
            .iter()
            .map(|t| TrailerValue {
                name: t.name.clone(),
                url: t.url.clone(),
                quality: t.quality.clone(),
                language: t.language.clone(),
                in_nfo: t.in_nfo,
            })
            .collect(),
    )
}

/// A stored rating value as the user typed it: the `f32` 7.8 reads 7.8, not 7.800000190734863.
fn rating_number(value: f32) -> f64 {
    value
        .to_string()
        .parse()
        .unwrap_or_else(|_| f64::from(value))
}

/// The source ratings, without the user's own rating (that is `user_rating`).
fn ratings_value(ratings: &Ratings) -> FieldValue {
    FieldValue::Ratings(
        ratings
            .iter()
            .filter(|r| !r.is_user())
            .map(|r| RatingValue {
                source: r.id.clone(),
                value: rating_number(r.value),
                max: r.max_value,
                votes: r.votes,
            })
            .collect(),
    )
}

fn season_text(show: &cv_domain::ShowFields, season: u32) -> Option<&SeasonText> {
    show.season_texts
        .iter()
        .find(|t| t.group == show.episode_group && t.season == season)
}

/// The current value of the field `key`, or `None` when the item has no such field.
pub fn read_field(item: &Item, key: &str) -> Option<FieldValue> {
    let core = &item.core;
    let value = match (key, &item.detail) {
        ("title", _) => FieldValue::Text(core.title.clone()),
        ("original_title", _) => FieldValue::Text(core.original_title.clone()),
        ("plot", _) => FieldValue::Text(core.plot.clone()),
        ("outline", _) => FieldValue::Text(core.outline.clone()),
        ("note", _) => FieldValue::Text(core.note.clone()),
        ("tags", _) => FieldValue::List(core.tags.iter().map(str::to_owned).collect()),
        ("studios", _) => FieldValue::List(core.production_companies.clone()),
        ("user_rating", _) => FieldValue::Number(core.ratings.user_rating().map(f64::from)),
        ("ratings", _) => ratings_value(&core.ratings),
        ("ids", _) => FieldValue::Pairs(
            core.ids
                .iter()
                .map(|(k, v)| Pair {
                    key: k.to_owned(),
                    value: v.to_owned(),
                })
                .collect(),
        ),
        ("cast", _) => credits_value(item, true),
        ("crew", _) => credits_value(item, false),
        (_, _) if key.starts_with(ARTWORK_PREFIX) => {
            let name = &key[ARTWORK_PREFIX.len()..];
            let MetaField::Artwork(kind) = MetaField::from_key(name)? else {
                return None;
            };
            FieldValue::Text(core.artwork_urls.get(&kind).cloned().unwrap_or_default())
        }
        ("sort_title", ItemDetail::Movie(m)) => FieldValue::Text(m.sort_title.clone()),
        ("sort_title", ItemDetail::Set(s)) => FieldValue::Text(s.sort_title.clone()),
        ("sort_title", ItemDetail::Show(s)) => FieldValue::Text(s.sort_title.clone()),
        ("tagline", ItemDetail::Movie(m)) => FieldValue::Text(m.tagline.clone()),
        ("tagline", ItemDetail::Show(s)) => FieldValue::Text(s.tagline.clone()),
        ("year", ItemDetail::Movie(_) | ItemDetail::Show(_)) => count(u32::from(core.year)),
        ("release_date", ItemDetail::Movie(m)) => date_text(m.release_date),
        ("first_aired", ItemDetail::Show(s)) => date_text(s.first_aired),
        ("first_aired", ItemDetail::Episode(e)) => date_text(e.first_aired),
        ("play_count", ItemDetail::Movie(m)) => count(m.play_count),
        ("play_count", ItemDetail::Episode(e)) => count(e.play_count),
        ("last_watched", ItemDetail::Movie(m)) => watched_day(m.last_watched_ms),
        ("last_watched", ItemDetail::Episode(e)) => watched_day(e.last_watched_ms),
        ("status", ItemDetail::Show(s)) => FieldValue::Text(status_name(s.status).to_owned()),
        ("runtime", ItemDetail::Movie(m)) => count(m.runtime_minutes),
        ("runtime", ItemDetail::Show(s)) => count(s.runtime_minutes),
        ("runtime", ItemDetail::Episode(e)) => count(e.runtime_minutes),
        ("certification", ItemDetail::Movie(m)) => certification_text(&m.certification),
        ("certification", ItemDetail::Show(s)) => certification_text(&s.certification),
        ("genres", ItemDetail::Movie(m)) => genre_names(&m.genres),
        ("genres", ItemDetail::Show(s)) => genre_names(&s.genres),
        ("countries", ItemDetail::Movie(m)) => FieldValue::List(m.countries.clone()),
        ("country", ItemDetail::Show(s)) => FieldValue::Text(s.country.clone()),
        ("spoken_languages", ItemDetail::Movie(m)) => FieldValue::List(m.spoken_languages.clone()),
        ("edition", ItemDetail::Movie(m)) => edition_text(&m.edition),
        ("edition", ItemDetail::Episode(e)) => edition_text(&e.edition),
        ("media_source", ItemDetail::Movie(m)) => source_text(&m.media_source),
        ("media_source", ItemDetail::Episode(e)) => source_text(&e.media_source),
        ("video_in_3d", ItemDetail::Movie(m)) => FieldValue::Flag(m.video_in_3d),
        ("top250", ItemDetail::Movie(m)) => count(m.top250),
        ("top250", ItemDetail::Show(s)) => count(s.top250),
        ("trailers", ItemDetail::Movie(m)) => trailers_value(&m.trailers),
        ("trailers", ItemDetail::Show(s)) => trailers_value(&s.trailers),
        ("numbers", ItemDetail::Episode(e)) => FieldValue::Numbers(
            e.numbers
                .iter()
                .map(|n| NumberValue {
                    ordering: ordering_name(n.group.ordering).to_owned(),
                    name: n.group.name.clone(),
                    season: n.season,
                    episode: n.episode,
                })
                .collect(),
        ),
        (_, ItemDetail::Show(show)) => {
            if let Some(n) = key.strip_prefix(SEASON_NAME_PREFIX) {
                let text = season_text(show, n.parse().ok()?)?;
                FieldValue::Text(text.name.clone())
            } else {
                let n = key.strip_prefix(SEASON_OVERVIEW_PREFIX)?;
                let text = season_text(show, n.parse().ok()?)?;
                FieldValue::Text(text.overview.clone())
            }
        }
        _ => return None,
    };
    Some(value)
}

/// Every field of the item with its value and lock key.
pub fn read_fields(item: &Item) -> Vec<FieldEntry> {
    let kind = item.kind();
    field_keys(item)
        .into_iter()
        .filter_map(|key| {
            let value = read_field(item, &key)?;
            let lock = lock_key(kind, &key).map(str::to_owned);
            Some(FieldEntry { key, value, lock })
        })
        .collect()
}

// ---- Applying a value -------------------------------------------------------------------------

fn want_text<'a>(key: &str, value: &'a FieldValue) -> Result<&'a str, FieldError> {
    match value {
        FieldValue::Text(text) => Ok(text.trim()),
        _ => Err(refuse(key, "expected text")),
    }
}

fn want_list<'a>(key: &str, value: &'a FieldValue) -> Result<&'a [String], FieldError> {
    match value {
        FieldValue::List(list) => Ok(list),
        _ => Err(refuse(key, "expected a list")),
    }
}

fn want_whole(
    key: &str,
    value: &FieldValue,
    max: u32,
    what: &str,
) -> Result<Option<u32>, FieldError> {
    let FieldValue::Number(number) = value else {
        return Err(refuse(key, "expected a number"));
    };
    let Some(number) = number else {
        return Ok(None);
    };
    if !number.is_finite() || number.fract() != 0.0 || *number < 0.0 || *number > f64::from(max) {
        return Err(refuse(
            key,
            format!("{what} must be a whole number from 0 to {max}"),
        ));
    }
    // Checked above: finite, whole and within `0..=max`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(Some(*number as u32))
}

fn want_date(key: &str, value: &FieldValue) -> Result<Option<CalendarDate>, FieldError> {
    let FieldValue::Date(date) = value else {
        return Err(refuse(key, "expected a date"));
    };
    match date.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        None => Ok(None),
        Some(text) => text
            .parse()
            .map(Some)
            .map_err(|_| refuse(key, "enter the date as year-month-day, such as 2024-03-01")),
    }
}

fn clean_list(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in list {
        let entry = entry.trim();
        if !entry.is_empty() && !out.iter().any(|e| e.eq_ignore_ascii_case(entry)) {
            out.push(entry.to_owned());
        }
    }
    out
}

fn genres_from(list: &[String]) -> Vec<Genre> {
    let mut out: Vec<Genre> = Vec::new();
    for entry in list {
        if let Some(genre) = Genre::parse(entry)
            && !out.contains(&genre)
        {
            out.push(genre);
        }
    }
    out
}

fn want_web_address(key: &str, text: &str) -> Result<(), FieldError> {
    if text.is_empty() || text.starts_with("http://") || text.starts_with("https://") {
        Ok(())
    } else {
        Err(refuse(
            key,
            "enter a web address that starts with http:// or https://",
        ))
    }
}

fn require_title(kind: ItemKind, key: &str, text: &str) -> Result<(), FieldError> {
    if text.is_empty() && kind != ItemKind::Episode {
        Err(refuse(key, "a title is required"))
    } else {
        Ok(())
    }
}

fn apply_people(
    item: &mut Item,
    key: &str,
    value: &FieldValue,
    cast: bool,
) -> Result<(), FieldError> {
    let FieldValue::People(people) = value else {
        return Err(refuse(key, "expected a list of people"));
    };
    let old = std::mem::take(&mut item.core.credits);
    let (mut kept_cast, mut kept_crew): (Vec<Credit>, Vec<Credit>) =
        old.iter().cloned().partition(|c| is_cast(c.kind));
    let mut pool: Vec<Credit> = if cast {
        kept_cast.clone()
    } else {
        kept_crew.clone()
    };
    let mut next: Vec<Credit> = Vec::new();
    for person in people {
        let name = person.name.trim();
        if name.is_empty() {
            continue;
        }
        let mut kind = parse_kind(&person.kind);
        if cast && !is_cast(kind) {
            kind = CreditKind::Actor;
        } else if !cast && is_cast(kind) {
            kind = CreditKind::Other;
        }
        // Keep what is known about a person who stays in the list: IDs and photo addresses.
        let known = pool
            .iter()
            .position(|c| c.person.name == name)
            .map(|i| pool.remove(i));
        let mut credit = Credit::new(kind, name, person.role.trim());
        if let Some(known) = known {
            credit.person = known.person;
        }
        next.push(credit);
    }
    if cast {
        kept_cast = next;
    } else {
        kept_crew = next;
    }
    kept_cast.extend(kept_crew);
    item.core.credits = kept_cast;
    Ok(())
}

fn apply_trailers(
    trailers: &mut Vec<Trailer>,
    key: &str,
    value: &FieldValue,
) -> Result<(), FieldError> {
    let FieldValue::Trailers(list) = value else {
        return Err(refuse(key, "expected a list of trailers"));
    };
    let mut pool = std::mem::take(trailers);
    let mut next = Vec::new();
    for entry in list {
        let url = entry.url.trim();
        if url.is_empty() {
            continue;
        }
        want_web_address(key, url)?;
        let known = pool
            .iter()
            .position(|t| t.url == url)
            .map(|i| pool.remove(i));
        let mut trailer = known.unwrap_or_else(|| Trailer {
            provider: "manual".to_owned(),
            ..Trailer::default()
        });
        url.clone_into(&mut trailer.url);
        entry.name.trim().clone_into(&mut trailer.name);
        entry.quality.trim().clone_into(&mut trailer.quality);
        entry.language.trim().clone_into(&mut trailer.language);
        trailer.in_nfo = entry.in_nfo;
        if trailer.id.is_empty() {
            trailer.id = format!("manual-{}", next.len() + 1);
        }
        next.push(trailer);
    }
    *trailers = next;
    Ok(())
}

fn apply_numbers(
    numbers: &mut EpisodeNumbers,
    key: &str,
    value: &FieldValue,
) -> Result<(), FieldError> {
    let FieldValue::Numbers(list) = value else {
        return Err(refuse(key, "expected episode numbers"));
    };
    let mut next = EpisodeNumbers::new();
    for entry in list {
        let ordering = parse_ordering(&entry.ordering)
            .ok_or_else(|| refuse(key, "unknown episode ordering"))?;
        let group = EpisodeGroup::new(ordering, entry.name.trim());
        if next.get(&group).is_some() {
            return Err(refuse(key, "each ordering can appear only once"));
        }
        next.set(EpisodeNumber::new(group, entry.season, entry.episode));
    }
    *numbers = next;
    Ok(())
}

/// Replaces the source ratings with the list; the user's own rating stays. A rating whose value,
/// scale and votes did not change keeps the time it was fetched; a new or changed one is stamped
/// with the time it is stored.
fn apply_ratings(ratings: &mut Ratings, key: &str, value: &FieldValue) -> Result<(), FieldError> {
    let FieldValue::Ratings(list) = value else {
        return Err(refuse(key, "expected ratings"));
    };
    let mut next = Ratings::new();
    if let Some(user) = ratings.get(rating_key::USER) {
        next.insert(user.clone());
    }
    for entry in list {
        let source = entry.source.trim();
        if source.is_empty() {
            return Err(refuse(key, "each rating needs a source"));
        }
        let id = canonical_rating_id(source);
        if id == rating_key::USER {
            return Err(refuse(key, "your own rating goes in Your rating"));
        }
        if next.get(&id).is_some() {
            return Err(refuse(key, format!("{id} is listed twice")));
        }
        if entry.max == 0 {
            return Err(refuse(key, format!("the scale of {id} must be above 0")));
        }
        if !entry.value.is_finite() || entry.value < 0.0 || entry.value > f64::from(entry.max) {
            return Err(refuse(
                key,
                format!("the {id} rating must be from 0 to {}", entry.max),
            ));
        }
        // Checked above: finite and within 0..=max, which an f32 holds.
        #[allow(clippy::cast_possible_truncation)]
        let mut rating =
            Rating::new(id.clone(), entry.value as f32, entry.votes, entry.max).normalised();
        if let Some(old) = ratings.get(&id) {
            // The title's page on the source's site stays when a value is edited.
            rating.url.clone_from(&old.url);
        }
        if let Some(old) = ratings.get(&id)
            && old.value.to_bits() == rating.value.to_bits()
            && old.max_value == rating.max_value
            && old.votes == rating.votes
        {
            rating.fetched_at_ms = old.fetched_at_ms;
        }
        next.insert(rating);
    }
    *ratings = next;
    Ok(())
}

fn apply_ids(ids: &mut ProviderIds, key: &str, value: &FieldValue) -> Result<(), FieldError> {
    let FieldValue::Pairs(pairs) = value else {
        return Err(refuse(key, "expected IDs"));
    };
    let mut next = ProviderIds::new();
    for pair in pairs {
        let (name, id) = (pair.key.trim(), pair.value.trim());
        if name.is_empty() && id.is_empty() {
            continue;
        }
        if name.is_empty() || id.is_empty() {
            return Err(refuse(key, "each ID needs a source and a value"));
        }
        next.set(name, id);
    }
    *ids = next;
    Ok(())
}

fn apply_season_text(
    show: &mut cv_domain::ShowFields,
    season: u32,
    name: Option<&str>,
    overview: Option<&str>,
) {
    let group = show.episode_group.clone();
    let index = show
        .season_texts
        .iter()
        .position(|t| t.group == group && t.season == season)
        .unwrap_or_else(|| {
            show.season_texts.push(SeasonText {
                group,
                season,
                ..SeasonText::default()
            });
            show.season_texts.len() - 1
        });
    let slot = &mut show.season_texts[index];
    if let Some(name) = name {
        name.clone_into(&mut slot.name);
    }
    if let Some(overview) = overview {
        overview.clone_into(&mut slot.overview);
    }
}

/// Writes one value into the item after checking it.
///
/// # Errors
/// Returns the reason when the value is refused; the item is unchanged then.
pub fn apply_change(item: &mut Item, key: &str, value: &FieldValue) -> Result<(), FieldError> {
    let kind = item.kind();
    if !field_keys(item).iter().any(|k| k == key) && !is_new_season_key(item, key) {
        return Err(refuse(key, "this field cannot be edited here"));
    }
    match key {
        "title" => {
            let text = want_text(key, value)?;
            require_title(kind, key, text)?;
            text.clone_into(&mut item.core.title);
        }
        "original_title" => want_text(key, value)?.clone_into(&mut item.core.original_title),
        "plot" => want_text(key, value)?.clone_into(&mut item.core.plot),
        "outline" => want_text(key, value)?.clone_into(&mut item.core.outline),
        "note" => want_text(key, value)?.clone_into(&mut item.core.note),
        "tags" => item.core.tags = Tags::from(clean_list(want_list(key, value)?)),
        "studios" => item.core.production_companies = clean_list(want_list(key, value)?),
        "user_rating" => {
            let FieldValue::Number(rating) = value else {
                return Err(refuse(key, "expected a number"));
            };
            match rating {
                None => item.core.ratings.clear_user_rating(),
                Some(v) => {
                    #[allow(clippy::cast_possible_truncation)]
                    item.core
                        .ratings
                        .set_user_rating(*v as f32)
                        .map_err(|e| refuse(key, e.to_string()))?;
                }
            }
        }
        "ids" => apply_ids(&mut item.core.ids, key, value)?,
        "ratings" => apply_ratings(&mut item.core.ratings, key, value)?,
        "cast" => apply_people(item, key, value, true)?,
        "crew" => apply_people(item, key, value, false)?,
        "year" => {
            let FieldValue::Number(number) = value else {
                return Err(refuse(key, "expected a number"));
            };
            let range = f64::from(YEAR_MIN)..=f64::from(YEAR_MAX);
            item.core.year = match number {
                None => 0,
                Some(n) if *n == 0.0 => 0,
                // Checked: whole and within the range, so it fits.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                Some(n) if n.fract() == 0.0 && range.contains(n) => *n as u16,
                Some(_) => return Err(refuse(key, YEAR_RULE)),
            };
        }
        _ if key.starts_with(ARTWORK_PREFIX) => {
            let text = want_text(key, value)?;
            want_web_address(key, text)?;
            let Some(MetaField::Artwork(kind)) = MetaField::from_key(&key[ARTWORK_PREFIX.len()..])
            else {
                return Err(refuse(key, "unknown artwork type"));
            };
            if text.is_empty() {
                item.core.artwork_urls.remove(&kind);
            } else {
                item.core.artwork_urls.insert(kind, text.to_owned());
            }
        }
        _ => apply_kind_change(item, key, value)?,
    }
    Ok(())
}

fn is_new_season_key(item: &Item, key: &str) -> bool {
    matches!(item.detail, ItemDetail::Show(_))
        && (key
            .strip_prefix(SEASON_NAME_PREFIX)
            .is_some_and(|n| n.parse::<u32>().is_ok())
            || key
                .strip_prefix(SEASON_OVERVIEW_PREFIX)
                .is_some_and(|n| n.parse::<u32>().is_ok()))
}

fn apply_kind_change(item: &mut Item, key: &str, value: &FieldValue) -> Result<(), FieldError> {
    match &mut item.detail {
        ItemDetail::Movie(m) => apply_movie(m, key, value),
        ItemDetail::Set(s) => match key {
            "sort_title" => {
                want_text(key, value)?.clone_into(&mut s.sort_title);
                Ok(())
            }
            _ => Err(refuse(key, "this field cannot be edited here")),
        },
        ItemDetail::Show(s) => apply_show(s, key, value),
        ItemDetail::Episode(e) => apply_episode(e, key, value),
        ItemDetail::Season(_) => Err(refuse(key, "this field cannot be edited here")),
    }
}

/// A stored certification as the user reads and types it: `US: R` for the token `US_R`. The
/// country stays, because a short form alone (`PG`) is used by more than one country.
fn certification_text(certification: &Certification) -> FieldValue {
    FieldValue::Text(cv_nfo::format_certification(
        &certification.0,
        cv_nfo::CertificationStyle::Medium,
    ))
}

/// Typed certification text as the stored token: `US: R`, `R` and `US_R` are all `US_R`. Text
/// that names no known rating is kept as written.
fn certification_from(text: &str) -> Certification {
    Certification(cv_nfo::parse_certification(text))
}

fn apply_movie(
    m: &mut cv_domain::MovieFields,
    key: &str,
    value: &FieldValue,
) -> Result<(), FieldError> {
    match key {
        "sort_title" => want_text(key, value)?.clone_into(&mut m.sort_title),
        "tagline" => want_text(key, value)?.clone_into(&mut m.tagline),
        "release_date" => m.release_date = want_date(key, value)?,
        "runtime" => {
            m.runtime_minutes = want_whole(key, value, 100_000, "the runtime")?.unwrap_or(0)
        }
        "certification" => m.certification = certification_from(want_text(key, value)?),
        "play_count" => {
            m.play_count = want_whole(key, value, 100_000, "the play count")?.unwrap_or(0);
        }
        "last_watched" => m.last_watched_ms = watched_instant(key, value, m.last_watched_ms)?,
        "genres" => m.genres = genres_from(want_list(key, value)?),
        "countries" => m.countries = clean_list(want_list(key, value)?),
        "spoken_languages" => m.spoken_languages = clean_list(want_list(key, value)?),
        "edition" => m.edition = Edition::parse(want_text(key, value)?).unwrap_or_default(),
        "media_source" => {
            m.media_source = MediaSource::parse(want_text(key, value)?).unwrap_or_default();
        }
        "video_in_3d" => match value {
            FieldValue::Flag(flag) => m.video_in_3d = *flag,
            _ => return Err(refuse(key, "expected on or off")),
        },
        "top250" => m.top250 = want_whole(key, value, 250, "the rank")?.unwrap_or(0),
        "trailers" => apply_trailers(&mut m.trailers, key, value)?,
        _ => return Err(refuse(key, "this field cannot be edited here")),
    }
    Ok(())
}

fn apply_show(
    s: &mut cv_domain::ShowFields,
    key: &str,
    value: &FieldValue,
) -> Result<(), FieldError> {
    match key {
        "sort_title" => want_text(key, value)?.clone_into(&mut s.sort_title),
        "tagline" => want_text(key, value)?.clone_into(&mut s.tagline),
        "first_aired" => s.first_aired = want_date(key, value)?,
        "status" => {
            s.status = match want_text(key, value)?.to_ascii_lowercase().as_str() {
                "" | "unknown" => AiredStatus::Unknown,
                "continuing" => AiredStatus::Continuing,
                "ended" => AiredStatus::Ended,
                "canceled" | "cancelled" => AiredStatus::Canceled,
                _ => return Err(refuse(key, "choose continuing, ended or canceled")),
            };
        }
        "runtime" => {
            s.runtime_minutes = want_whole(key, value, 100_000, "the runtime")?.unwrap_or(0)
        }
        "certification" => s.certification = certification_from(want_text(key, value)?),
        "genres" => s.genres = genres_from(want_list(key, value)?),
        "country" => want_text(key, value)?.clone_into(&mut s.country),
        "top250" => s.top250 = want_whole(key, value, 250, "the rank")?.unwrap_or(0),
        "trailers" => apply_trailers(&mut s.trailers, key, value)?,
        _ => {
            if let Some(n) = key
                .strip_prefix(SEASON_NAME_PREFIX)
                .and_then(|n| n.parse().ok())
            {
                let text = want_text(key, value)?.to_owned();
                apply_season_text(s, n, Some(&text), None);
            } else if let Some(n) = key
                .strip_prefix(SEASON_OVERVIEW_PREFIX)
                .and_then(|n| n.parse().ok())
            {
                let text = want_text(key, value)?.to_owned();
                apply_season_text(s, n, None, Some(&text));
            } else {
                return Err(refuse(key, "this field cannot be edited here"));
            }
        }
    }
    Ok(())
}

fn apply_episode(
    e: &mut cv_domain::EpisodeFields,
    key: &str,
    value: &FieldValue,
) -> Result<(), FieldError> {
    match key {
        "first_aired" => e.first_aired = want_date(key, value)?,
        "play_count" => {
            e.play_count = want_whole(key, value, 100_000, "the play count")?.unwrap_or(0);
        }
        "last_watched" => e.last_watched_ms = watched_instant(key, value, e.last_watched_ms)?,
        "runtime" => {
            e.runtime_minutes = want_whole(key, value, 100_000, "the runtime")?.unwrap_or(0)
        }
        "edition" => e.edition = Edition::parse(want_text(key, value)?).unwrap_or_default(),
        "media_source" => {
            e.media_source = MediaSource::parse(want_text(key, value)?).unwrap_or_default();
        }
        "numbers" => apply_numbers(&mut e.numbers, key, value)?,
        _ => return Err(refuse(key, "this field cannot be edited here")),
    }
    Ok(())
}
