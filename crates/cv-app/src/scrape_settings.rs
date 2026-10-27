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
//!
//! The field keys are `cv_domain::MetaField::key`. A missing value means every field a scrape
//! may write. Tags are not one of them: a provider's tags are its keywords (hundreds for a small
//! library, "albuquerque, new mexico"), and they are never imported, so the tags a title has from
//! its NFO stay as they are (`MetaField::is_scrapable`). A stored `tags` key is ignored, like any
//! unknown key, and a key that does not belong to the kind is dropped. The same options apply to
//! a rescrape of one item and to a batch scrape.

use std::collections::BTreeMap;

use cv_domain::{EpisodeOrdering, FieldSet, ItemKind, ScrapeOptions};
use serde_json::Value;

type Values = BTreeMap<String, Value>;

/// The setting that chooses the ordering new shows start in.
pub const EPISODE_ORDER_KEY: &str = "tv.episode-order";

/// The setting that switches automatic collection assignment after a scrape.
pub const AUTO_GROUP_KEY: &str = "movies.collections.auto-group";

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
}

impl Default for ScrapeSettings {
    fn default() -> Self {
        Self::from_values(&Values::new())
    }
}

/// The fields a scrape of `kind` writes when the user has not chosen: every field a scrape may
/// write (never Tags).
#[must_use]
pub fn default_fields(kind: ItemKind) -> FieldSet {
    scrapable(FieldSet::all_for(kind))
}

/// `fields` without the ones a scrape never writes.
fn scrapable(fields: FieldSet) -> FieldSet {
    fields.iter().filter(|f| f.is_scrapable()).collect()
}

fn fields(values: &Values, key: &str, kind: ItemKind) -> FieldSet {
    let all = FieldSet::all_for(kind);
    match values.get(key).and_then(Value::as_str) {
        Some(text) => scrapable(
            FieldSet::from_keys(text)
                .iter()
                .filter(|f| all.contains(*f))
                .collect(),
        ),
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
        }
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
    fn the_defaults_select_every_field_a_scrape_may_write() {
        let s = ScrapeSettings::default();
        assert!(!s.do_not_overwrite);
        assert!(s.auto_group_collections);
        assert_eq!(s.episode_order, EpisodeOrdering::Aired);
        assert_eq!(
            s.movie.to_keys(),
            "title,original_title,tagline,plot,year,release_date,rating,top250,runtime,\
             certification,genres,spoken_languages,country,studio,collection,actors,crew,\
             trailer,poster,fanart,banner,clearart,disc,clearlogo,thumb,keyart,squareart,\
             extrafanart,extrathumb"
        );
        assert_eq!(
            s.show.to_keys(),
            "title,original_title,tagline,plot,year,release_date,status,rating,top250,runtime,\
             certification,genres,country,studio,actors,crew,trailer,poster,fanart,banner,\
             clearart,disc,clearlogo,thumb,characterart,keyart,squareart,extrafanart"
        );
        assert_eq!(
            s.episode.to_keys(),
            "title,original_title,plot,release_date,rating,runtime,studio,actors,crew,thumb"
        );
    }

    #[test]
    fn tags_are_never_selected_even_when_a_stored_list_names_them() {
        let s = ScrapeSettings::from_values(&values(&[(
            "metadata.scrape-fields-movie",
            json!("title,tags,genres"),
        )]));
        assert_eq!(s.movie.to_keys(), "title,genres");
        assert!(
            !s.options_for(ItemKind::Set)
                .fields
                .contains(MetaField::Tags)
        );
        assert!(!MetaField::Tags.is_scrapable());
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
    fn wrong_types_fall_back_to_the_defaults() {
        let s = ScrapeSettings::from_values(&values(&[
            ("metadata.scrape-fields-show", json!(5)),
            ("metadata.scrape-do-not-overwrite", json!("yes")),
        ]));
        assert_eq!(s, ScrapeSettings::default());
    }
}
