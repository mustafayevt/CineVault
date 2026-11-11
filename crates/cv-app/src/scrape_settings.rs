//! The scrape options, read from the settings map.
//!
//! The keys are the ids of the settings page (`ui/src/features/settings/schema.ts`).
//!
//! | Key | Values | Meaning |
//! |---|---|---|
//! | `metadata.scrape-fields-movie` | comma-separated field keys (default: every field) | the fields a movie scrape may write |
//! | `metadata.scrape-fields-show` | same | the fields a show scrape may write |
//! | `metadata.scrape-fields-episode` | same | the fields an episode scrape may write |
//! | `metadata.scrape-do-not-overwrite` | bool (default off) | write only fields that are empty |
//! | `tv.episode-order` | `aired` (default), `dvd` or `absolute` | the ordering a show starts in when it is first scraped |
//! | `movies.collections.auto-group` | bool (default on) | put a scraped movie into its TMDB collection |
//! | `metadata.original-title-languages` | comma-separated language codes (default none) | a movie or show in one of these original languages starts with its original title as its title, when it is scraped for the first time |
//!
//! The field keys are `cv_domain::MetaField::key`. A missing value means every field, Tags
//! included: a scrape adds the provider's keywords to the tags a title has. An unknown key, or
//! one that does not belong to the kind, is dropped. The same options apply to a rescrape of one
//! item and to a batch scrape.

use std::collections::BTreeMap;

use cv_domain::{EpisodeOrdering, FieldSet, ItemKind, ScrapeOptions};
use serde_json::Value;

type Values = BTreeMap<String, Value>;

/// The setting that chooses the ordering new shows start in.
pub const EPISODE_ORDER_KEY: &str = "tv.episode-order";

/// The setting that switches automatic collection assignment after a scrape.
pub const AUTO_GROUP_KEY: &str = "movies.collections.auto-group";

/// The setting that lists the original languages whose titles keep their original title.
pub const ORIGINAL_TITLE_LANGUAGES_KEY: &str = "metadata.original-title-languages";

/// The scrape options of the three scraped kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrapeSettings {
    pub movie: FieldSet,
    pub show: FieldSet,
    pub episode: FieldSet,
    pub do_not_overwrite: bool,
    /// `movies.collections.auto-group`: a movie scrape that writes the collection field also puts
    /// the movie into its collection. Off leaves collections to "Find collections" and by hand.
    pub auto_group_collections: bool,
    /// `tv.episode-order`: the ordering a new show starts in. A show already scraped keeps the
    /// ordering it has; the picker on the show page changes one show.
    pub episode_order: EpisodeOrdering,
    /// `metadata.original-title-languages`: two-letter codes of the original languages whose
    /// movies and shows start with their original title as their title. Empty by default.
    pub original_title_languages: Vec<String>,
}

impl Default for ScrapeSettings {
    fn default() -> Self {
        Self::from_values(&Values::new())
    }
}

/// The fields a scrape of `kind` writes when the user has not chosen: every field.
#[must_use]
pub fn default_fields(kind: ItemKind) -> FieldSet {
    FieldSet::all_for(kind)
}

fn fields(values: &Values, key: &str, kind: ItemKind) -> FieldSet {
    let all = FieldSet::all_for(kind);
    match values.get(key).and_then(Value::as_str) {
        Some(text) => FieldSet::from_keys(text)
            .iter()
            .filter(|f| all.contains(*f))
            .collect(),
        None => default_fields(kind),
    }
}

impl ScrapeSettings {
    /// Reads the settings; anything missing takes its default.
    pub fn from_values(values: &Values) -> Self {
        Self {
            movie: fields(values, "metadata.scrape-fields-movie", ItemKind::Movie),
            show: fields(values, "metadata.scrape-fields-show", ItemKind::Show),
            episode: fields(values, "metadata.scrape-fields-episode", ItemKind::Episode),
            do_not_overwrite: values
                .get("metadata.scrape-do-not-overwrite")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            auto_group_collections: values
                .get(AUTO_GROUP_KEY)
                .and_then(Value::as_bool)
                .unwrap_or(true),
            episode_order: match values.get(EPISODE_ORDER_KEY).and_then(Value::as_str) {
                Some("dvd") => EpisodeOrdering::Dvd,
                Some("absolute") => EpisodeOrdering::Absolute,
                _ => EpisodeOrdering::Aired,
            },
            original_title_languages: language_list(
                values
                    .get(ORIGINAL_TITLE_LANGUAGES_KEY)
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
        }
    }

    /// Whether a title whose original language is `language` starts with its original title.
    #[must_use]
    pub fn keeps_original_title(&self, language: &str) -> bool {
        let code = cv_domain::language_code(language);
        !code.is_empty() && self.original_title_languages.contains(&code)
    }

    /// The options for scraping an item of `kind`. Sets and seasons write everything they have.
    pub fn options_for(&self, kind: ItemKind) -> ScrapeOptions {
        let fields = match kind {
            ItemKind::Movie => self.movie.clone(),
            ItemKind::Show => self.show.clone(),
            ItemKind::Episode => self.episode.clone(),
            ItemKind::Set | ItemKind::Season => default_fields(kind),
        };
        ScrapeOptions {
            fields,
            do_not_overwrite: self.do_not_overwrite,
        }
    }
}

/// `fr, es` as `["fr", "es"]`: two-letter codes, without repeats or blanks.
fn language_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for code in text
        .split([',', ' ', ';'])
        .map(cv_domain::language_code)
        .filter(|code| !code.is_empty())
    {
        if !out.contains(&code) {
            out.push(code);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_domain::MetaField;
    use serde_json::json;

    fn values(pairs: &[(&str, Value)]) -> Values {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    /// `schema.ts` pins the same lists (`scrapeFieldDefaults` in `scrape.test.ts`).
    #[test]
    fn the_defaults_select_every_field() {
        let s = ScrapeSettings::default();
        assert!(!s.do_not_overwrite);
        assert!(s.auto_group_collections);
        assert_eq!(s.episode_order, EpisodeOrdering::Aired);
        assert_eq!(
            s.movie.to_keys(),
            "title,original_title,tagline,plot,year,release_date,rating,top250,runtime,\
             certification,genres,spoken_languages,country,studio,tags,collection,actors,crew,\
             trailer,poster,fanart,banner,clearart,disc,clearlogo,thumb,keyart,squareart,\
             extrafanart,extrathumb"
        );
        assert_eq!(
            s.show.to_keys(),
            "title,original_title,tagline,plot,year,release_date,status,rating,top250,runtime,\
             certification,genres,country,studio,tags,actors,crew,trailer,poster,fanart,banner,\
             clearart,disc,clearlogo,thumb,characterart,keyart,squareart,extrafanart"
        );
        assert_eq!(
            s.episode.to_keys(),
            "title,original_title,plot,release_date,rating,runtime,studio,tags,actors,crew,thumb"
        );
    }

    #[test]
    fn tags_are_a_field_a_stored_list_can_name_or_leave_out() {
        let s = ScrapeSettings::from_values(&values(&[
            ("metadata.scrape-fields-movie", json!("title,tags,genres")),
            ("metadata.scrape-fields-show", json!("title,genres")),
        ]));
        assert!(
            s.options_for(ItemKind::Movie)
                .fields
                .contains(MetaField::Tags)
        );
        assert!(
            !s.options_for(ItemKind::Show)
                .fields
                .contains(MetaField::Tags)
        );
        assert!(
            ScrapeSettings::default()
                .options_for(ItemKind::Set)
                .fields
                .contains(MetaField::Tags)
        );
    }

    #[test]
    fn a_chosen_list_and_the_mode_are_read() {
        let s = ScrapeSettings::from_values(&values(&[
            (
                "metadata.scrape-fields-movie",
                json!("rating,fanart,status,bogus"),
            ),
            ("metadata.scrape-fields-episode", json!("")),
            ("metadata.scrape-do-not-overwrite", json!(true)),
        ]));
        assert_eq!(
            s.movie.to_keys(),
            "rating,fanart",
            "wrong-kind and unknown keys drop"
        );
        assert!(s.episode.is_empty());
        assert!(s.do_not_overwrite);
        let options = s.options_for(ItemKind::Movie);
        assert!(options.do_not_overwrite && options.fields.contains(MetaField::Rating));
        assert!(!s.options_for(ItemKind::Set).fields.is_empty());
    }

    #[test]
    fn automatic_collection_grouping_can_be_switched_off() {
        let off = ScrapeSettings::from_values(&values(&[(AUTO_GROUP_KEY, json!(false))]));
        assert!(!off.auto_group_collections);
        let on = ScrapeSettings::from_values(&values(&[(AUTO_GROUP_KEY, json!(true))]));
        assert!(on.auto_group_collections);
    }

    #[test]
    fn the_default_episode_order_is_read() {
        for (text, want) in [
            ("dvd", EpisodeOrdering::Dvd),
            ("absolute", EpisodeOrdering::Absolute),
            ("aired", EpisodeOrdering::Aired),
            ("bogus", EpisodeOrdering::Aired),
        ] {
            let s = ScrapeSettings::from_values(&values(&[(EPISODE_ORDER_KEY, json!(text))]));
            assert_eq!(s.episode_order, want, "{text}");
        }
    }

    #[test]
    fn the_original_title_languages_are_read_as_codes() {
        assert!(
            ScrapeSettings::default()
                .original_title_languages
                .is_empty()
        );
        let s = ScrapeSettings::from_values(&values(&[(
            ORIGINAL_TITLE_LANGUAGES_KEY,
            json!("fr, ES,fra,,pt-BR"),
        )]));
        assert_eq!(s.original_title_languages, ["fr", "es", "pt"]);
        assert!(s.keeps_original_title("fr"));
        assert!(s.keeps_original_title("spa"));
        assert!(!s.keeps_original_title("en"));
        assert!(!s.keeps_original_title(""));
    }

    #[test]
    fn wrong_types_fall_back_to_the_defaults() {
        let s = ScrapeSettings::from_values(&values(&[
            ("metadata.scrape-fields-show", json!(5)),
            ("metadata.scrape-do-not-overwrite", json!("yes")),
        ]));
        assert_eq!(s, ScrapeSettings::default());
    }
}
