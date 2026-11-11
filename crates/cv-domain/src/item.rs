//! Library items: movies, movie sets, shows, seasons and episodes.
//!
//! Every item has an [`ItemCore`] with the fields all kinds share, plus an [`ItemDetail`] with
//! the fields of its kind.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    CalendarDate, Credit, Edition, EpisodeGroup, EpisodeNumbers, Genre, MediaFile, MediaFileType,
    MediaSource, ProviderIds, Ratings, Trailer,
};

/// The database identity of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ItemId(pub i64);

/// The kind of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Movie,
    Set,
    Show,
    Season,
    Episode,
}

/// A parental-guidance rating as a canonical token such as `US_PG13` or `DE_FSK12`.
///
/// Empty means unknown. The token set is large and provider-specific, so it is kept as text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Certification(pub String);

/// Broadcast status of a show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiredStatus {
    #[default]
    Unknown,
    Continuing,
    Ended,
    Canceled,
}

/// One filename change in a folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RenameStep {
    pub old_filename: String,
    pub new_filename: String,
}

/// What the renamer last did to an item's folder and files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RenameHistory {
    pub old_path: String,
    pub new_path: String,
    pub files: Vec<RenameStep>,
}

/// Tags, unique ignoring case; the first spelling wins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<String>", into = "Vec<String>")]
pub struct Tags(Vec<String>);

impl Tags {
    /// Adds a tag. Returns `false` when it is blank or already present (ignoring case).
    pub fn add(&mut self, tag: &str) -> bool {
        let tag = tag.trim();
        if tag.is_empty() || self.contains(tag) {
            return false;
        }
        self.0.push(tag.to_owned());
        true
    }

    pub fn contains(&self, tag: &str) -> bool {
        self.0
            .iter()
            .any(|t| t.to_lowercase() == tag.trim().to_lowercase())
    }

    pub fn remove(&mut self, tag: &str) -> bool {
        let before = self.0.len();
        let wanted = tag.trim().to_lowercase();
        self.0.retain(|t| t.to_lowercase() != wanted);
        before != self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<String>> for Tags {
    fn from(tags: Vec<String>) -> Self {
        let mut out = Self::default();
        for t in &tags {
            out.add(t);
        }
        out
    }
}

impl From<Tags> for Vec<String> {
    fn from(tags: Tags) -> Self {
        tags.0
    }
}

/// Fields shared by every item kind.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ItemCore {
    /// Entity-level lock: blocks scraping and merging changes.
    pub locked: bool,
    /// Root path of the data source the item lives in.
    pub data_source: String,
    pub ids: ProviderIds,
    /// The title CineVault uses everywhere: lists, title pages, `${title}` in the naming
    /// templates, the NFO `<title>`. The original title while [`Self::use_original_title`] is on.
    pub title: String,
    pub original_title: String,
    /// The title chosen to be the original title (movies and shows only): `title` then holds the
    /// original title and the title in the metadata language is kept in `localized_title`.
    pub use_original_title: bool,
    /// The title in the metadata language, kept while [`Self::use_original_title`] is on so that
    /// switching back restores it. Empty otherwise.
    pub localized_title: String,
    /// The production's own language as a lower-case ISO 639-1 code (`en`, `fr`), empty when
    /// unknown. Kept for movies and shows; an episode reads its show's.
    pub original_language: String,
    /// Release year, `0` when unknown.
    pub year: u16,
    pub plot: String,
    pub outline: String,
    /// The item's folder.
    pub path: String,
    /// When the item was added, epoch milliseconds.
    pub date_added_ms: i64,
    pub production_companies: Vec<String>,
    /// The user's own note.
    pub note: String,
    pub ratings: Ratings,
    pub media_files: Vec<MediaFile>,
    pub tags: Tags,
    /// Remote artwork URLs by artwork type.
    pub artwork_urls: BTreeMap<MediaFileType, String>,
    /// The video filename at first import.
    pub original_filename: String,
    pub last_scraper_id: String,
    pub last_scrape_language: String,
    pub rename_history: Option<RenameHistory>,
    /// Cast and crew, in display order.
    pub credits: Vec<Credit>,
}

impl ItemCore {
    /// The title in the metadata language: the title, or the one kept aside while the original
    /// title is used.
    #[must_use]
    pub fn localized_title(&self) -> &str {
        if self.use_original_title {
            &self.localized_title
        } else {
            &self.title
        }
    }

    /// Sets the title in the metadata language. While the original title is used it is kept
    /// aside and the title stays the original one.
    pub fn set_localized_title(&mut self, title: &str) {
        if self.use_original_title {
            title.clone_into(&mut self.localized_title);
        } else {
            title.clone_into(&mut self.title);
        }
    }

    /// Sets the original title. While it is used as the title the title follows it; an empty
    /// original title switches back to the localized title, so the title is never emptied.
    pub fn set_original_title(&mut self, original: &str) {
        original.clone_into(&mut self.original_title);
        if !self.use_original_title {
            return;
        }
        if original.trim().is_empty() {
            self.choose_original_title(false);
        } else {
            original.clone_into(&mut self.title);
        }
    }

    /// Whether there is an original title to choose: one that differs from the title in the
    /// metadata language.
    #[must_use]
    pub fn offers_original_title(&self) -> bool {
        let original = self.original_title.trim();
        !original.is_empty() && original != self.localized_title().trim()
    }

    /// Uses the original title as the title (`true`) or goes back to the localized title
    /// (`false`). Neither value is lost: the one not in use is kept. Returns whether the choice
    /// changed; choosing the original title of a title without a different one changes nothing.
    pub fn choose_original_title(&mut self, original: bool) -> bool {
        if original == self.use_original_title {
            return false;
        }
        if original {
            if !self.offers_original_title() {
                return false;
            }
            self.localized_title = std::mem::take(&mut self.title);
            self.title.clone_from(&self.original_title);
            self.use_original_title = true;
        } else {
            let localized = std::mem::take(&mut self.localized_title);
            if !localized.trim().is_empty() {
                self.title = localized;
            }
            self.use_original_title = false;
        }
        true
    }

    /// For writers that set the title as a whole (reading an NFO): when the title is no longer
    /// the original title, the choice ends and the title stays as written.
    pub fn settle_title_choice(&mut self) {
        if self.use_original_title
            && (self.original_title.trim().is_empty() || self.title != self.original_title)
        {
            self.use_original_title = false;
            self.localized_title.clear();
        }
    }
}

/// A movie that belongs to a set but is not in the library.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MissingMovie {
    pub title: String,
    pub original_title: String,
    pub year: u16,
    pub release_date: Option<CalendarDate>,
    pub plot: String,
    pub ids: ProviderIds,
    /// A poster image the provider offers, shown for the ghosted entry. Empty when unknown.
    pub poster_url: String,
}

/// An episode that exists at the provider but not in the library.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MissingEpisode {
    pub title: String,
    pub numbers: EpisodeNumbers,
    pub first_aired: Option<CalendarDate>,
    pub plot: String,
}

/// A season name and overview in one ordering.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeasonText {
    pub group: EpisodeGroup,
    pub season: u32,
    pub name: String,
    pub overview: String,
}

/// Movie-only fields.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MovieFields {
    pub english_title: String,
    pub sort_title: String,
    pub tagline: String,
    pub runtime_minutes: u32,
    pub watched: bool,
    pub play_count: u32,
    pub last_watched_ms: Option<i64>,
    /// A disc-structure movie (`VIDEO_TS` / `BDMV`).
    pub is_disc: bool,
    pub spoken_languages: Vec<String>,
    pub countries: Vec<String>,
    pub release_date: Option<CalendarDate>,
    /// Several movies share one folder.
    pub multi_movie_dir: bool,
    /// Rank in the top-250 list, `0` when unranked.
    pub top250: u32,
    pub media_source: MediaSource,
    pub video_in_3d: bool,
    pub certification: Certification,
    pub edition: Edition,
    /// The movie is split over several files (CD1/CD2).
    pub stacked: bool,
    pub genres: Vec<Genre>,
    pub extra_thumbs: Vec<String>,
    pub extra_fanarts: Vec<String>,
    pub trailers: Vec<Trailer>,
    /// Titles of linked TV shows.
    pub show_links: Vec<String>,
}

/// Movie-set (collection) fields. Members are the movies whose `parent_id` is the set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SetFields {
    pub english_title: String,
    pub sort_title: String,
    /// Movies of the collection that are not in the library.
    pub missing_movies: Vec<MissingMovie>,
}

/// Show-only fields.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ShowFields {
    pub english_title: String,
    pub sort_title: String,
    pub tagline: String,
    pub runtime_minutes: u32,
    pub year_end: u16,
    pub first_aired: Option<CalendarDate>,
    pub status: AiredStatus,
    pub country: String,
    pub certification: Certification,
    /// The ordering used to display episode numbers.
    pub episode_group: EpisodeGroup,
    /// Orderings the provider offers.
    pub episode_groups: Vec<EpisodeGroup>,
    pub genres: Vec<Genre>,
    pub top250: u32,
    pub extra_fanart_urls: Vec<String>,
    pub trailers: Vec<Trailer>,
    pub season_texts: Vec<SeasonText>,
    pub missing_episodes: Vec<MissingEpisode>,
    /// The provider's episode list has been fetched at least once, so `missing_episodes` says
    /// what the library lacks. Before that an empty list means nothing.
    pub episode_list_fetched: bool,
}

/// Season-only fields. A season belongs to a show through `Item::parent_id`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeasonFields {
    /// Season number; `0` is the specials season.
    pub season: u32,
}

/// Episode-only fields. An episode belongs to a show through `Item::parent_id`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EpisodeFields {
    pub english_title: String,
    /// Season and episode number per ordering.
    pub numbers: EpisodeNumbers,
    pub first_aired: Option<CalendarDate>,
    pub is_disc: bool,
    /// One file holds several episodes.
    pub multi_episode: bool,
    pub watched: bool,
    pub play_count: u32,
    pub last_watched_ms: Option<i64>,
    pub media_source: MediaSource,
    pub edition: Edition,
    pub stacked: bool,
    pub runtime_minutes: u32,
}

/// The kind-specific part of an item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemDetail {
    Movie(MovieFields),
    Set(SetFields),
    Show(ShowFields),
    Season(SeasonFields),
    Episode(EpisodeFields),
}

impl ItemDetail {
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Movie(_) => ItemKind::Movie,
            Self::Set(_) => ItemKind::Set,
            Self::Show(_) => ItemKind::Show,
            Self::Season(_) => ItemKind::Season,
            Self::Episode(_) => ItemKind::Episode,
        }
    }
}

/// A library item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// Database id; `None` until the item is stored.
    #[serde(default)]
    pub id: Option<ItemId>,
    /// The owner: the show for a season or episode, the set for a movie in a set.
    #[serde(default)]
    pub parent_id: Option<ItemId>,
    #[serde(default)]
    pub core: ItemCore,
    pub detail: ItemDetail,
}

impl Item {
    pub fn new(detail: ItemDetail) -> Self {
        Self {
            id: None,
            parent_id: None,
            core: ItemCore::default(),
            detail,
        }
    }

    pub fn movie(title: impl Into<String>) -> Self {
        let mut item = Self::new(ItemDetail::Movie(MovieFields::default()));
        item.core.title = title.into();
        item
    }

    pub fn set(title: impl Into<String>) -> Self {
        let mut item = Self::new(ItemDetail::Set(SetFields::default()));
        item.core.title = title.into();
        item
    }

    pub fn show(title: impl Into<String>) -> Self {
        let mut item = Self::new(ItemDetail::Show(ShowFields::default()));
        item.core.title = title.into();
        item
    }

    pub fn season(show: ItemId, season: u32) -> Self {
        let mut item = Self::new(ItemDetail::Season(SeasonFields { season }));
        item.parent_id = Some(show);
        item
    }

    pub fn episode(show: ItemId, title: impl Into<String>) -> Self {
        let mut item = Self::new(ItemDetail::Episode(EpisodeFields::default()));
        item.parent_id = Some(show);
        item.core.title = title.into();
        item
    }

    pub fn kind(&self) -> ItemKind {
        self.detail.kind()
    }

    /// Whether the item can carry a choice of title: movies and shows.
    #[must_use]
    pub fn has_title_choice(&self) -> bool {
        matches!(self.kind(), ItemKind::Movie | ItemKind::Show)
    }

    /// [`ItemCore::choose_original_title`] for a movie or a show; other kinds never use the
    /// original title.
    pub fn choose_original_title(&mut self, original: bool) -> bool {
        if original && !self.has_title_choice() {
            return false;
        }
        self.core.choose_original_title(original)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreditKind, EpisodeNumber, EpisodeOrdering, Rating, id_key, rating_key};

    fn sleeping_city() -> Item {
        let mut item = Item::movie("The Sleeping City");
        item.core.original_title = "La Ciudad Dormida".into();
        item
    }

    #[test]
    fn choosing_the_original_title_keeps_the_localized_one() {
        let mut item = sleeping_city();
        assert!(item.core.offers_original_title());
        assert!(item.choose_original_title(true));
        assert_eq!(item.core.title, "La Ciudad Dormida");
        assert_eq!(item.core.localized_title(), "The Sleeping City");
        assert!(!item.choose_original_title(true), "already chosen");
        assert!(item.choose_original_title(false));
        assert_eq!(item.core.title, "The Sleeping City");
        assert_eq!(item.core.localized_title, "");
        assert!(!item.core.use_original_title);
    }

    #[test]
    fn a_title_without_another_original_title_has_no_choice() {
        let mut same = Item::movie("Heat");
        same.core.original_title = "Heat".into();
        assert!(!same.core.offers_original_title());
        assert!(!same.choose_original_title(true));
        let mut none = Item::movie("Heat");
        assert!(!none.choose_original_title(true));
        let mut episode = Item::episode(ItemId(1), "Pilot");
        episode.core.original_title = "Piloto".into();
        assert!(
            !episode.choose_original_title(true),
            "only movies and shows choose"
        );
    }

    #[test]
    fn writers_keep_the_choice_consistent() {
        let mut item = sleeping_city();
        item.choose_original_title(true);
        item.core.set_localized_title("The Sleeping Town");
        assert_eq!(item.core.title, "La Ciudad Dormida");
        assert_eq!(item.core.localized_title, "The Sleeping Town");
        item.core.set_original_title("La Ciudad Dormida 2");
        assert_eq!(item.core.title, "La Ciudad Dormida 2");
        // An emptied original title never empties the title: the localized one comes back.
        item.core.set_original_title("");
        assert!(!item.core.use_original_title);
        assert_eq!(item.core.title, "The Sleeping Town");
    }

    #[test]
    fn a_title_written_as_a_whole_ends_the_choice_when_it_is_not_the_original() {
        let mut item = sleeping_city();
        item.choose_original_title(true);
        item.core.settle_title_choice();
        assert!(item.core.use_original_title, "still the original title");
        item.core.title = "Something Else".into();
        item.core.settle_title_choice();
        assert!(!item.core.use_original_title);
        assert_eq!(item.core.title, "Something Else");
        assert_eq!(item.core.localized_title, "");
    }

    fn populated_core() -> ItemCore {
        let mut core = ItemCore {
            locked: true,
            data_source: "D:\\Movies".into(),
            title: "Alien".into(),
            original_title: "Alien".into(),
            year: 1979,
            plot: "In space...".into(),
            outline: "Short".into(),
            path: "D:\\Movies\\Alien (1979)".into(),
            date_added_ms: 1_700_000_000_000,
            production_companies: vec!["Brandywine".into(), "20th Century Fox".into()],
            note: "Rewatch".into(),
            original_filename: "alien.1979.mkv".into(),
            last_scraper_id: "tmdb".into(),
            last_scrape_language: "en".into(),
            rename_history: Some(RenameHistory {
                old_path: "D:\\Movies\\alien".into(),
                new_path: "D:\\Movies\\Alien (1979)".into(),
                files: vec![RenameStep {
                    old_filename: "alien.mkv".into(),
                    new_filename: "Alien (1979).mkv".into(),
                }],
            }),
            ..ItemCore::default()
        };
        core.ids.set(id_key::IMDB, "tt0078748");
        core.ids.set(id_key::TMDB, "348");
        core.ids.set("kinopoisk", "12345");
        core.ratings.insert(Rating::new("imdb", 8.5, 900_000, 10));
        core.ratings
            .insert(Rating::new(rating_key::USER, 10.0, 0, 10));
        core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            "D:\\Movies\\Alien (1979)",
            "a.mkv",
        ));
        core.tags.add("Space");
        core.tags.add("classic");
        core.artwork_urls.insert(
            MediaFileType::Poster,
            "https://example.test/poster.jpg".into(),
        );
        core.artwork_urls.insert(
            MediaFileType::SeasonPoster,
            "https://example.test/sp.jpg".into(),
        );
        core.credits
            .push(Credit::new(CreditKind::Actor, "Sigourney Weaver", "Ripley"));
        core.credits
            .push(Credit::new(CreditKind::Director, "Ridley Scott", ""));
        core
    }

    fn round_trip(item: &Item) -> Item {
        let json = serde_json::to_string(item).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn fully_populated_movie_round_trips() {
        let mut item = Item::movie("Alien");
        item.id = Some(ItemId(42));
        item.parent_id = Some(ItemId(7));
        item.core = populated_core();
        item.detail = ItemDetail::Movie(MovieFields {
            english_title: "Alien".into(),
            sort_title: "Alien 1".into(),
            tagline: "In space no one can hear you scream".into(),
            runtime_minutes: 117,
            watched: true,
            play_count: 3,
            last_watched_ms: Some(1_710_000_000_000),
            is_disc: false,
            spoken_languages: vec!["en".into()],
            countries: vec!["US".into(), "GB".into()],
            release_date: CalendarDate::new(1979, 5, 25),
            multi_movie_dir: false,
            top250: 120,
            media_source: MediaSource::UhdBluray,
            video_in_3d: false,
            certification: Certification("US_R".into()),
            edition: Edition::Other("Ultimate Archive".into()),
            stacked: true,
            genres: vec![
                Genre::Horror,
                Genre::ScienceFiction,
                Genre::Other("Space Opera".into()),
            ],
            extra_thumbs: vec!["u1".into()],
            extra_fanarts: vec!["u2".into()],
            trailers: vec![Trailer {
                name: "Trailer".into(),
                in_nfo: true,
                ..Trailer::default()
            }],
            show_links: vec!["Some Show".into()],
        });
        assert_eq!(round_trip(&item), item);
        assert_eq!(item.kind(), ItemKind::Movie);
    }

    #[test]
    fn dynamic_values_survive_inside_items() {
        let mut item = Item::movie("Alien");
        if let ItemDetail::Movie(m) = &mut item.detail {
            m.edition = Edition::parse("{Ultimate Archive}".trim_matches(['{', '}'])).unwrap();
            m.media_source = MediaSource::parse("Super 8 Scan").unwrap();
            m.genres = vec![
                Genre::parse("Cosmic Horror").unwrap(),
                Genre::parse("horror").unwrap(),
            ];
        }
        let back = round_trip(&item);
        let ItemDetail::Movie(m) = back.detail else {
            panic!("not a movie")
        };
        assert_eq!(m.edition, Edition::Other("Ultimate Archive".into()));
        assert_eq!(m.media_source, MediaSource::Other("Super 8 Scan".into()));
        assert_eq!(
            m.genres,
            vec![Genre::Other("Cosmic Horror".into()), Genre::Horror]
        );
    }

    #[test]
    fn show_season_episode_and_set_round_trip() {
        let mut show = Item::show("Severance");
        show.id = Some(ItemId(1));
        show.detail = ItemDetail::Show(ShowFields {
            status: AiredStatus::Continuing,
            first_aired: CalendarDate::new(2022, 2, 18),
            episode_group: EpisodeGroup::dvd(),
            episode_groups: vec![
                EpisodeGroup::aired(),
                EpisodeGroup::dvd(),
                EpisodeGroup::new(EpisodeOrdering::Alternate, "Production"),
            ],
            genres: vec![Genre::Drama, Genre::Other("Workplace".into())],
            season_texts: vec![SeasonText {
                group: EpisodeGroup::aired(),
                season: 1,
                name: "Season 1".into(),
                overview: "o".into(),
            }],
            missing_episodes: vec![MissingEpisode {
                title: "Future".into(),
                numbers: {
                    let mut n = EpisodeNumbers::new();
                    n.set(EpisodeNumber::aired(2, 10));
                    n
                },
                ..MissingEpisode::default()
            }],
            ..ShowFields::default()
        });
        assert_eq!(round_trip(&show), show);

        let season = Item::season(ItemId(1), 0);
        assert_eq!(round_trip(&season), season);
        assert_eq!(season.kind(), ItemKind::Season);

        let mut episode = Item::episode(ItemId(1), "Good News About Hell");
        if let ItemDetail::Episode(e) = &mut episode.detail {
            e.numbers.set(EpisodeNumber::aired(1, 1));
            e.numbers.set(EpisodeNumber::new(EpisodeGroup::dvd(), 1, 2));
            e.numbers
                .set(EpisodeNumber::new(EpisodeGroup::absolute(), 0, 1));
            e.edition = Edition::Remastered;
            e.multi_episode = true;
        }
        assert_eq!(round_trip(&episode), episode);
        assert_eq!(episode.parent_id, Some(ItemId(1)));

        let mut set = Item::set("Alien Collection");
        if let ItemDetail::Set(s) = &mut set.detail {
            s.missing_movies.push(MissingMovie {
                title: "Alien 9".into(),
                year: 2030,
                ..MissingMovie::default()
            });
        }
        assert_eq!(round_trip(&set), set);
    }

    #[test]
    fn tags_are_unique_ignoring_case_on_load() {
        let core: ItemCore =
            serde_json::from_str(r#"{"tags":["Space","space","  ","Classic","SPACE"]}"#).unwrap();
        assert_eq!(
            core.tags.iter().collect::<Vec<_>>(),
            vec!["Space", "Classic"]
        );
        assert!(core.tags.contains("CLASSIC"));
    }

    #[test]
    fn partial_documents_fill_defaults() {
        let item: Item =
            serde_json::from_str(r#"{"detail":{"kind":"movie","runtimeMinutes":90}}"#).unwrap();
        let ItemDetail::Movie(m) = &item.detail else {
            panic!("not a movie")
        };
        assert_eq!(m.runtime_minutes, 90);
        assert_eq!(m.media_source, MediaSource::Unknown);
        assert_eq!(m.edition, Edition::None);
        assert_eq!(item.id, None);
    }
}
