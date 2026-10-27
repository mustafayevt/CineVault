//! AniDB's anime XML to CineVault items, hits and episodes.
//!
//! One answer holds the whole anime: titles in many languages, creators, the description,
//! ratings, external resources, tags, characters with their voice actors and every episode. A
//! missing element is simply empty; an anime needs only its ID and a main title.

use cv_domain::{
    AiredStatus, CalendarDate, Credit, CreditKind, EpisodeFields, EpisodeGroup, EpisodeNumber,
    Genre, Item, ItemDetail, MediaFileType, ProviderIds, Rating, id_key,
};

use super::xml::{self, Node};
use super::{ID_KEY, IMAGE_BASE, PROVIDER_ID};
use crate::model::{RequestOptions, SearchHit};
use crate::shared::{parse_date, set_id, year_of};

/// The rating source key AniDB ratings are stored under.
pub const RATING_ID: &str = "anidb";

/// Tags below this weight are too weakly voted to use.
const MIN_TAG_WEIGHT: u32 = 200;

/// What an AniDB answer is.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Answer {
    Anime(Box<Anime>),
    /// AniDB's own error element: its code and message.
    Error {
        code: String,
        message: String,
    },
}

/// Reads an answer: an `<anime>` or an `<error>`.
pub(super) fn parse_answer(text: &str) -> Result<Answer, String> {
    let root = xml::parse(text)?;
    match root.name.as_str() {
        "anime" => Anime::from_node(root).map(|a| Answer::Anime(Box::new(a))),
        "error" => Ok(Answer::Error {
            code: root.attr("code").unwrap_or("").to_owned(),
            message: root.text,
        }),
        other => Err(format!("unexpected <{other}> element")),
    }
}

/// An anime as AniDB describes it.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Anime {
    root: Node,
    id: u64,
    main_title: String,
}

/// The titles of an anime (inside `<titles>`) or of an episode (directly inside it), as
/// `(language, type, text)`. Episode titles have no type.
fn titles(node: &Node) -> Vec<(String, String, String)> {
    node.child("titles")
        .unwrap_or(node)
        .all("title")
        .filter(|n| !n.text.is_empty())
        .map(|n| {
            (
                n.attr("xml:lang").unwrap_or("").to_ascii_lowercase(),
                n.attr("type").unwrap_or("").to_owned(),
                n.text.clone(),
            )
        })
        .collect()
}

fn primary(language: &str) -> &str {
    language.split(['-', '_']).next().unwrap_or("")
}

/// The first title of `kind` (`main`, `official`, ...) in `language`, matched on the primary
/// subtag so `zh` finds `zh-hans`.
fn title_in<'a>(
    list: &'a [(String, String, String)],
    language: &str,
    kinds: &[&str],
) -> Option<&'a str> {
    let wanted = primary(&language.to_ascii_lowercase()).to_owned();
    if wanted.is_empty() {
        return None;
    }
    kinds.iter().find_map(|kind| {
        list.iter()
            .find(|(lang, k, _)| k == kind && primary(lang) == wanted)
            .map(|(_, _, text)| text.as_str())
    })
}

/// The best title for the request: the preferred language, the fallback language, English, then
/// the main title.
fn pick_title<'a>(
    list: &'a [(String, String, String)],
    options: &RequestOptions,
    main: &'a str,
) -> &'a str {
    [
        options.language.as_str(),
        options.fallback_language.as_str(),
        "en",
    ]
    .iter()
    .find_map(|language| title_in(list, language, &["official", "synonym"]))
    .unwrap_or(main)
}

/// The episode title in the preferred language, the fallback language or English, else the first
/// one (AniDB lists the romaji title).
fn episode_title<'a>(list: &'a [(String, String, String)], options: &RequestOptions) -> &'a str {
    [
        options.language.as_str(),
        options.fallback_language.as_str(),
        "en",
    ]
    .iter()
    .find_map(|language| {
        let wanted = primary(&language.to_ascii_lowercase()).to_owned();
        list.iter()
            .find(|(lang, _, _)| !wanted.is_empty() && primary(lang) == wanted)
            .map(|(_, _, text)| text.as_str())
    })
    .or_else(|| list.first().map(|(_, _, text)| text.as_str()))
    .unwrap_or("")
}

impl Anime {
    fn from_node(root: Node) -> Result<Self, String> {
        let id = root
            .attr("id")
            .and_then(|id| id.trim().parse::<u64>().ok())
            .filter(|id| *id != 0)
            .ok_or("the anime has no ID")?;
        let list = titles(&root);
        let main_title = list
            .iter()
            .find(|(_, kind, _)| kind == "main")
            .or_else(|| list.first())
            .map(|(_, _, text)| text.clone())
            .ok_or("the anime has no title")?;
        Ok(Self {
            root,
            id,
            main_title,
        })
    }

    fn ids(&self) -> ProviderIds {
        let mut ids = ProviderIds::new();
        set_id(&mut ids, ID_KEY, self.id.to_string());
        for resource in self
            .root
            .child("resources")
            .into_iter()
            .flat_map(|r| r.all("resource"))
        {
            let identifier = resource
                .child("externalentity")
                .and_then(|e| e.child_text("identifier"))
                .unwrap_or("");
            match resource.attr("type") {
                Some("2") => set_id(&mut ids, "myanimelist", identifier),
                Some("43") => {
                    if let Some(imdb) = crate::shared::imdb_title_id(identifier) {
                        set_id(&mut ids, id_key::IMDB, imdb);
                    }
                }
                Some("44") => {
                    // The identifier is the number, or `movie/123` and `tv/123`.
                    let number = identifier.rsplit('/').next().unwrap_or("");
                    if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) {
                        set_id(&mut ids, id_key::TMDB, number);
                    }
                }
                _ => {}
            }
        }
        ids
    }

    fn start(&self) -> Option<CalendarDate> {
        self.root.child_text("startdate").and_then(parse_date)
    }

    fn end(&self) -> Option<CalendarDate> {
        self.root.child_text("enddate").and_then(parse_date)
    }

    fn picture(&self) -> Option<String> {
        self.root
            .child_text("picture")
            .map(|p| format!("{IMAGE_BASE}{p}"))
    }

    fn rating(&self) -> Option<Rating> {
        let ratings = self.root.child("ratings")?;
        ["permanent", "temporary"].iter().find_map(|kind| {
            let node = ratings.child(kind)?;
            let value: f32 = node.text.replace(',', ".").parse().ok()?;
            let votes: u32 = node.attr("count").and_then(|c| c.parse().ok()).unwrap_or(0);
            (value > 0.0).then(|| Rating::new(RATING_ID, value, votes, 10))
        })
    }

    /// The description without AniDB's markup and its `Source:` lines.
    fn plot(&self) -> String {
        self.root
            .child_text("description")
            .map(clean_description)
            .unwrap_or_default()
    }

    pub(super) fn build_hit(&self, options: &RequestOptions, exact_id: bool) -> SearchHit {
        let list = titles(&self.root);
        SearchHit {
            provider: PROVIDER_ID.to_owned(),
            ids: self.ids(),
            title: pick_title(&list, options, &self.main_title).to_owned(),
            original_title: title_in(&list, "ja", &["official"])
                .unwrap_or(&self.main_title)
                .to_owned(),
            year: year_of(self.start()),
            overview: self.plot(),
            poster_url: self.picture(),
            exact_id,
            popularity: 0.0,
            votes: 0,
        }
    }

    /// Weighty, non-spoiler tags as genres, heaviest first.
    fn genres(&self, limit: usize) -> Vec<Genre> {
        let mut tags: Vec<(u32, String)> = self
            .root
            .child("tags")
            .into_iter()
            .flat_map(|t| t.all("tag"))
            .filter(|t| {
                t.attr("globalspoiler") != Some("true") && t.attr("localspoiler") != Some("true")
            })
            .filter_map(|t| {
                let weight = t.attr("weight")?.parse::<u32>().ok()?;
                Some((weight, t.child_text("name")?.to_owned()))
            })
            .filter(|(weight, _)| *weight >= MIN_TAG_WEIGHT)
            .collect();
        tags.sort_by_key(|tag| std::cmp::Reverse(tag.0));
        let mut out: Vec<Genre> = Vec::new();
        for (_, name) in tags {
            if let Some(genre) = Genre::parse(&title_case(&name))
                && !out.contains(&genre)
            {
                out.push(genre);
            }
            if out.len() == limit {
                break;
            }
        }
        out
    }

    fn credits(&self) -> (Vec<Credit>, Vec<String>) {
        let mut credits: Vec<Credit> = Vec::new();
        let mut studios: Vec<String> = Vec::new();
        for creator in self
            .root
            .child("creators")
            .into_iter()
            .flat_map(|c| c.all("name"))
        {
            let name = creator.text.as_str();
            if name.is_empty() {
                continue;
            }
            let job = creator.attr("type").unwrap_or("");
            let kind = match job {
                "Animation Work" | "Work" => {
                    if !studios.iter().any(|s| s == name) {
                        studios.push(name.to_owned());
                    }
                    continue;
                }
                "Direction" | "Chief Direction" | "Series Direction" => CreditKind::Director,
                "Original Work" | "Original Plan" | "Series Composition" | "Script"
                | "Screenplay" => CreditKind::Writer,
                "Music" => CreditKind::Composer,
                _ => CreditKind::Other,
            };
            if !credits
                .iter()
                .any(|c| c.kind == kind && c.person.name == name && c.role == job)
            {
                let mut credit = Credit::new(kind, name, job);
                if let Some(id) = creator.attr("id") {
                    set_id(&mut credit.person.ids, ID_KEY, id);
                }
                credits.push(credit);
            }
        }
        // The voice actors, one credit per actor and character.
        for character in self
            .root
            .child("characters")
            .into_iter()
            .flat_map(|c| c.all("character"))
        {
            let (Some(actor), Some(role)) = (
                character.child("seiyuu").filter(|s| !s.text.is_empty()),
                character.child_text("name"),
            ) else {
                continue;
            };
            if credits.iter().any(|c| {
                c.kind == CreditKind::Actor && c.person.name == actor.text && c.role == role
            }) {
                continue;
            }
            let mut credit = Credit::new(CreditKind::Actor, &actor.text, role);
            if let Some(id) = actor.attr("id") {
                set_id(&mut credit.person.ids, ID_KEY, id);
            }
            if let Some(picture) = actor.attr("picture").filter(|p| !p.is_empty()) {
                credit.person.thumb_url = format!("{IMAGE_BASE}{picture}");
            }
            credits.push(credit);
        }
        (credits, studios)
    }

    fn episode_nodes(&self) -> impl Iterator<Item = &Node> {
        self.root
            .child("episodes")
            .into_iter()
            .flat_map(|e| e.all("episode"))
    }

    /// The length of the first regular episode, which is the running time of a film.
    fn first_length(&self) -> u32 {
        self.episode_nodes()
            .filter(|e| e.child("epno").and_then(|n| n.attr("type")) == Some("1"))
            .find_map(|e| e.child_text("length").and_then(|l| l.parse::<u32>().ok()))
            .unwrap_or(0)
    }

    /// The anime as a show (`movie` false) or a film.
    pub(super) fn build_item(&self, movie: bool, options: &RequestOptions) -> Item {
        let list = titles(&self.root);
        let title = pick_title(&list, options, &self.main_title).to_owned();
        let mut item = if movie {
            Item::movie(&title)
        } else {
            Item::show(&title)
        };
        let (credits, studios) = self.credits();
        let start = self.start();
        let end = self.end();
        {
            let core = &mut item.core;
            core.ids = self.ids();
            core.original_title = title_in(&list, "ja", &["official"])
                .unwrap_or(&self.main_title)
                .to_owned();
            core.year = year_of(start);
            core.plot = self.plot();
            core.credits = credits;
            core.production_companies = studios;
            if let Some(rating) = self.rating() {
                core.ratings.insert(rating);
            }
            if let Some(picture) = self.picture() {
                core.artwork_urls.insert(MediaFileType::Poster, picture);
            }
        }
        let english = title_in(&list, "en", &["official"])
            .unwrap_or("")
            .to_owned();
        match &mut item.detail {
            ItemDetail::Show(show) => {
                show.english_title = english;
                show.first_aired = start;
                show.year_end = year_of(end);
                show.status = if end.is_some() {
                    AiredStatus::Ended
                } else if start.is_some() {
                    AiredStatus::Continuing
                } else {
                    AiredStatus::Unknown
                };
                show.genres = self.genres(10);
                show.runtime_minutes = self.first_length();
                show.episode_group = EpisodeGroup::aired();
                show.episode_groups = vec![EpisodeGroup::aired(), EpisodeGroup::absolute()];
            }
            ItemDetail::Movie(film) => {
                film.english_title = english;
                film.release_date = start;
                film.genres = self.genres(20);
                film.runtime_minutes = self.first_length();
            }
            _ => {}
        }
        item
    }

    /// The episodes: regular ones in aired and absolute numbering (season 1, AniDB's own number),
    /// specials as season 0. Credits, trailers, parodies and other extras are left out.
    pub(super) fn episodes(&self, options: &RequestOptions) -> Vec<Item> {
        let mut rows: Vec<(u32, u32, Item)> = Vec::new();
        for node in self.episode_nodes() {
            let Some(number) = node.child("epno") else {
                continue;
            };
            let digits: String = number
                .text
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(char::is_ascii_digit)
                .collect();
            let Ok(episode) = digits.parse::<u32>() else {
                continue;
            };
            let special = match number.attr("type") {
                Some("1") => false,
                Some("2") => true,
                _ => continue,
            };
            let list = titles(node);
            let title = episode_title(&list, options);
            let aired = node.child_text("airdate").and_then(parse_date);
            let mut item = Item::new(ItemDetail::Episode(EpisodeFields::default()));
            item.core.title = title.to_owned();
            item.core.plot = node
                .child_text("summary")
                .map(clean_description)
                .unwrap_or_default();
            item.core.year = year_of(aired);
            if let Some(id) = node.attr("id") {
                set_id(&mut item.core.ids, ID_KEY, id);
            }
            if let Some(rating) = node.child("rating").and_then(|r| {
                let value: f32 = r.text.replace(',', ".").parse().ok()?;
                let votes: u32 = r.attr("votes").and_then(|v| v.parse().ok()).unwrap_or(0);
                (value > 0.0).then(|| Rating::new(RATING_ID, value, votes, 10))
            }) {
                item.core.ratings.insert(rating);
            }
            let season = u32::from(!special);
            if let ItemDetail::Episode(fields) = &mut item.detail {
                fields.first_aired = aired;
                fields.runtime_minutes = node
                    .child_text("length")
                    .and_then(|l| l.parse().ok())
                    .unwrap_or(0);
                fields
                    .numbers
                    .set(EpisodeNumber::new(EpisodeGroup::aired(), season, episode));
                if !special {
                    fields
                        .numbers
                        .set(EpisodeNumber::new(EpisodeGroup::absolute(), 1, episode));
                }
            }
            rows.push((season, episode, item));
        }
        rows.sort_by_key(|(season, episode, _)| (*season, *episode));
        rows.into_iter().map(|(_, _, item)| item).collect()
    }
}

/// `science fiction` to `Science Fiction`.
fn title_case(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect::<String>()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// AniDB's description markup to plain text: `http://anidb.net/ch101 [Mika]` becomes `Mika`,
/// `[i]` and similar tags are dropped, and the `Source:` lines at the end are removed.
pub(super) fn clean_description(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = ["http://anidb.net/", "https://anidb.net/"]
        .iter()
        .filter_map(|p| rest.find(p))
        .min()
    {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let url_end = after.find(char::is_whitespace).unwrap_or(after.len());
        let tail = &after[url_end..];
        let label = tail.trim_start_matches([' ', '\t']);
        match label
            .strip_prefix('[')
            .and_then(|l| l.find(']').map(|e| (&l[..e], &l[e + 1..])))
        {
            Some((label, remainder)) => {
                out.push_str(label);
                rest = remainder;
            }
            None => {
                out.push_str(&after[..url_end]);
                rest = tail;
            }
        }
    }
    out.push_str(rest);

    let mut stripped = String::with_capacity(out.len());
    let mut chars = out.as_str();
    while let Some(open) = chars.find('[') {
        stripped.push_str(&chars[..open]);
        let tail = &chars[open..];
        let tag_end = tail.find(']');
        let is_markup = tag_end.is_some_and(|end| {
            let name = tail[1..end].trim_start_matches('/').to_ascii_lowercase();
            matches!(name.as_str(), "i" | "b" | "u" | "s" | "quote" | "spoiler")
        });
        match (is_markup, tag_end) {
            (true, Some(end)) => chars = &tail[end + 1..],
            _ => {
                stripped.push('[');
                chars = &tail[1..];
            }
        }
    }
    stripped.push_str(chars);

    stripped
        .lines()
        .map(str::trim_end)
        .filter(|line| {
            !line
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("source:")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_lose_links_markup_and_sources() {
        assert_eq!(
            clean_description(
                "A crew.\nhttp://anidb.net/ch101 [Captain Mika] meets [i]the stowaway[/i] [sic].\nhttps://anidb.net/a1 stays.\nSource: Example Press\nNote: Based on a novel."
            ),
            "A crew.\nCaptain Mika meets the stowaway [sic].\nhttps://anidb.net/a1 stays.\nNote: Based on a novel."
        );
        assert_eq!(clean_description("Plain."), "Plain.");
        assert_eq!(clean_description("Source: Only"), "");
        assert_eq!(clean_description("Unclosed [i tag"), "Unclosed [i tag");
    }

    #[test]
    fn tag_names_become_title_case() {
        assert_eq!(title_case("science fiction"), "Science Fiction");
        assert_eq!(title_case("  space   travel "), "Space Travel");
        assert_eq!(title_case(""), "");
    }

    #[test]
    fn answers_are_anime_or_errors() {
        assert!(matches!(
            parse_answer(r#"<error code="500">banned</error>"#),
            Ok(Answer::Error { code, message }) if code == "500" && message == "banned"
        ));
        assert!(parse_answer("<anime><titles/></anime>").is_err());
        assert!(
            parse_answer(r#"<anime id="1"><titles><title xml:lang="x-jat" type="main">A</title></titles></anime>"#)
                .is_ok()
        );
        assert!(parse_answer("<html></html>").is_err());
        assert!(parse_answer("not xml").is_err());
    }

    #[test]
    fn titles_follow_the_language_then_the_fallback() {
        let list = vec![
            ("x-jat".to_owned(), "main".to_owned(), "Hoshi".to_owned()),
            ("en".to_owned(), "official".to_owned(), "Star".to_owned()),
            ("de".to_owned(), "official".to_owned(), "Stern".to_owned()),
            ("zh-hans".to_owned(), "official".to_owned(), "星".to_owned()),
        ];
        let mut options = RequestOptions::default();
        assert_eq!(pick_title(&list, &options, "Hoshi"), "Star");
        options.language = "de-DE".to_owned();
        assert_eq!(pick_title(&list, &options, "Hoshi"), "Stern");
        options.language = "zh".to_owned();
        assert_eq!(pick_title(&list, &options, "Hoshi"), "星");
        options.language = "tr".to_owned();
        options.fallback_language = "fr".to_owned();
        assert_eq!(pick_title(&list, &options, "Hoshi"), "Star");
        assert_eq!(pick_title(&[], &options, "Hoshi"), "Hoshi");
    }
}
