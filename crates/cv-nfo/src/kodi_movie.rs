//! The Kodi movie NFO writer, and the Emby and Jellyfin variants that only differ in a few
//! places (they extend the Kodi layout).
//!
//! The element order, value formats and layout are the ones in `tests/compat/expected`:
//!
//! ```text
//! title originaltitle sorttitle epbookmark year ratings userrating [top250] set plot [outline]
//! tagline runtime [thumb*] [fanart] mpaa certification id tmdbid uniqueid* country* status code
//! premiered watched playcount [lastplayed] genre* studio* credits* director* tag* actor*
//! producer* [trailer] languages showlink* [dateadded] [lockdata] [fileinfo]
//! [videoassettitle hasvideoversions] <unmanaged elements> <comment>
//! source edition original_filename user_note english_title [crc32] crew* cv_locked
//! ```
//!
//! Elements without a value are written empty (`<year/>`), except those marked optional.
//!
//! Emby differs from Kodi in three places: `<set>` carries `tmdbcolid` and is followed by the other
//! `<set>` entries of the old NFO, and `credits` and `director` come after the actors (before the
//! producers). Jellyfin leaves out `epbookmark`, `top250`, `status` and `code`, and writes
//! `<collectionnumber>` (the TMDB collection ID) after the edition elements.

use cv_domain::{AudioStream, Credit, CreditKind, Edition, Person, Rating, Trailer};

use crate::format;
use crate::locale;
use crate::movie::{DateAddedField, MovieNfo, MovieNfoOptions, MovieSetRef};
use crate::streams;
use crate::xml::{Document, Element};

/// The rating key written for the TMDB source (Kodi's `themoviedb`).
pub(crate) fn rating_name(id: &str) -> &str {
    if id == "tmdb" { "themoviedb" } else { id }
}

/// The layouts that follow the Kodi element order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KodiFlavor {
    Kodi,
    Emby,
    Jellyfin,
}

impl KodiFlavor {
    fn label(self) -> &'static str {
        match self {
            Self::Kodi => "Kodi",
            Self::Emby => "Emby",
            Self::Jellyfin => "Jellyfin",
        }
    }
}

/// Builds the Kodi NFO document of a movie.
pub fn movie_document(movie: &MovieNfo, options: &MovieNfoOptions) -> Document {
    kodi_family_document(KodiFlavor::Kodi, movie, options)
}

/// Builds the Kodi, Emby or Jellyfin NFO document of a movie.
pub(crate) fn kodi_family_document(
    flavor: KodiFlavor,
    movie: &MovieNfo,
    options: &MovieNfoOptions,
) -> Document {
    let mut root = Element::new("movie");
    let text = |name: &str, value: &str| Element::text(name, value);
    let jellyfin = flavor == KodiFlavor::Jellyfin;
    let emby = flavor == KodiFlavor::Emby;

    root.push(text("title", &movie.title));
    root.push(text("originaltitle", &movie.original_title));
    root.push(text("sorttitle", &movie.sort_title));
    if !jellyfin {
        root.push(text("epbookmark", &movie.carried.epbookmark));
    }
    root.push(text("year", &number_or_empty(movie.year)));
    root.push(ratings(movie, options));
    root.push(text("userrating", &user_rating(movie)));
    if movie.top250 > 0 && !jellyfin {
        root.push(text("top250", &movie.top250.to_string()));
    }
    root.push(movie_set(movie.movie_set.as_ref(), emby));
    if emby {
        for other in other_sets(movie) {
            root.push(movie_set(Some(other), true));
        }
    }
    root.push(text("plot", &movie.plot));
    if let Some(outline) = outline(movie, options) {
        root.push(text("outline", &outline));
    }
    root.push(text("tagline", &movie.tagline));
    root.push(text("runtime", &movie.runtime_minutes.to_string()));
    if options.artwork_urls {
        artwork(movie, &mut root);
    }

    let certification = crate::certification::format_certification(
        &movie.certification.0,
        options.certification_style,
    );
    root.push(text("mpaa", &certification));
    root.push(text("certification", &certification));
    root.push(text("id", movie.ids.get("imdb").unwrap_or_default()));
    root.push(text("tmdbid", movie.ids.get("tmdb").unwrap_or_default()));
    unique_ids(movie, &mut root);
    for country in non_blank(&movie.countries) {
        root.push(text("country", country));
    }
    if !jellyfin {
        root.push(text("status", &movie.carried.status));
        root.push(text("code", &movie.carried.code));
    }
    root.push(text(
        "premiered",
        &movie
            .release_date
            .map(|d| d.to_string())
            .unwrap_or_default(),
    ));
    root.push(text(
        "watched",
        if movie.watched { "true" } else { "false" },
    ));
    root.push(text("playcount", &play_count(movie).to_string()));
    if let Some(last_played) = last_played(movie) {
        root.push(text("lastplayed", &last_played));
    }
    for genre in &movie.genres {
        root.push(text("genre", &locale::genre_name(genre, &options.language)));
    }
    let studios = non_blank(&movie.studios);
    let studios = if options.single_studio {
        studios.take(1).collect::<Vec<_>>()
    } else {
        studios.collect()
    };
    for studio in studios {
        root.push(text("studio", studio));
    }
    if !emby {
        writers_and_directors(movie, &mut root);
    }
    for tag in non_blank(&movie.tags) {
        root.push(text("tag", tag));
    }
    for credit in credits_of(movie, CreditKind::Actor) {
        root.push(actor("actor", credit, options, true));
    }
    if emby {
        writers_and_directors(movie, &mut root);
    }
    for credit in credits_of(movie, CreditKind::Producer) {
        root.push(producer(credit, options));
    }
    if options.trailer {
        root.push(text("trailer", &trailer(&movie.trailers)));
    }
    root.push(text("languages", &languages(movie, options)));
    for link in non_blank(&movie.show_links) {
        root.push(text("showlink", link));
    }
    if options.write_date_added {
        root.push(text(
            "dateadded",
            &date_added(movie, options).unwrap_or_default(),
        ));
    }
    if options.lockdata {
        root.push(text("lockdata", "true"));
    }
    if options.fileinfo {
        root.push(fileinfo(movie, options));
    }
    if movie.edition != Edition::None {
        root.push(text("videoassettitle", &edition_title(&movie.edition)));
        root.push(text("hasvideoversions", "true"));
    }
    if jellyfin && let Some(id) = movie.ids.get(cv_domain::id_key::TMDB_SET) {
        root.push(text("collectionnumber", id));
    }
    for unknown in &movie.unknown_elements {
        root.push(unknown.clone());
    }

    private_tags(movie, options, &mut root);
    finish(root, options, flavor.label())
}

/// Renders the Kodi NFO of a movie: UTF-8 text, two-space indentation, CRLF line ends.
pub fn render_movie_nfo(movie: &MovieNfo, options: &MovieNfoOptions) -> String {
    movie_document(movie, options).render()
}

/// `<credits>` (writers) then `<director>`, one element per person.
fn writers_and_directors(movie: &MovieNfo, root: &mut Element) {
    for credit in credits_of(movie, CreditKind::Writer) {
        root.push(named_person("credits", credit));
    }
    for credit in credits_of(movie, CreditKind::Director) {
        root.push(named_person("director", credit));
    }
}

/// CineVault's own tags that close every movie NFO: the meta-data comment, the source, edition,
/// original file name, note and English title, the checksum, the crew and the lock element.
pub(crate) fn private_tags(movie: &MovieNfo, options: &MovieNfoOptions, root: &mut Element) {
    let text = |name: &str, value: &str| Element::text(name, value);
    root.push_comment(format!("{} meta data", product_name(&options.generator)));
    root.push(text("source", movie.media_source.name()));
    root.push(text("edition", movie.edition.name()));
    root.push(text("original_filename", &movie.original_filename));
    root.push(text("user_note", &movie.note));
    root.push(text("english_title", &movie.english_title));
    if let Some(crc) = movie.crc32.as_deref().filter(|c| !c.is_empty()) {
        root.push(text("crc32", crc));
    }
    if options.cv_extensions {
        extension_tags(movie, root);
    }
    for credit in movie.credits.iter().filter(|c| c.kind != CreditKind::Actor) {
        root.push(crew(credit, options));
    }
    root.push(text("cv_locked", &movie.locked_fields.join(",")));
}

/// CineVault's additions among the private tags: the original language, the 3D flag and the
/// extra thumbnails. Each is left out when it has no value.
fn extension_tags(movie: &MovieNfo, root: &mut Element) {
    push_text(root, "original_language", &movie.original_language);
    if movie.video_in_3d {
        root.push(Element::text("video3d", "true"));
    }
    let thumbs: Vec<&str> = movie
        .artwork
        .extra_thumbs
        .iter()
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
        .collect();
    if !thumbs.is_empty() {
        let mut element = Element::new("extrathumbs");
        for url in thumbs {
            element.push(Element::text("thumb", url));
        }
        root.push(element);
    }
}

/// Wraps the root into a document with the generator's header comment.
pub(crate) fn finish(root: Element, options: &MovieNfoOptions, connector: &str) -> Document {
    let mut document = Document::new(root);
    document.comment = Some(header_comment(options, connector));
    document
}

/// The first comment of an NFO: `created on <time> by <generator> for <connector>`.
pub(crate) fn header_comment(options: &MovieNfoOptions, connector: &str) -> String {
    if options.created_on.is_empty() {
        format!("created by {} for {connector}", options.generator)
    } else {
        format!(
            "created on {} by {} for {connector}",
            options.created_on, options.generator
        )
    }
}

/// The generator name without its version: `CineVault 0.1.0` gives `CineVault`.
pub(crate) fn product_name(generator: &str) -> &str {
    generator.split_whitespace().next().unwrap_or("CineVault")
}

pub(crate) fn number_or_empty(value: u32) -> String {
    if value == 0 {
        String::new()
    } else {
        value.to_string()
    }
}

pub(crate) fn non_blank(list: &[String]) -> impl Iterator<Item = &str> {
    list.iter().map(|s| s.trim()).filter(|s| !s.is_empty())
}

// ---- ratings ---------------------------------------------------------------------------------

fn ratings(movie: &MovieNfo, options: &MovieNfoOptions) -> Element {
    let mut element = Element::new("ratings");
    let main = main_rating(movie, options);
    if let Some(main) = main {
        element.push(rating_element(main, true));
    }
    let mut others: Vec<&Rating> = external_ratings(movie)
        .filter(|r| main.is_none_or(|m| m.id != r.id))
        .collect();
    format::stable_map_order(&mut others, |r| rating_name(&r.id).to_owned());
    for other in others {
        element.push(rating_element(other, false));
    }
    element
}

/// Every rating except the user rating.
fn external_ratings(movie: &MovieNfo) -> impl Iterator<Item = &Rating> {
    movie
        .ratings
        .iter()
        .filter(|r| r.id != cv_domain::rating_key::USER)
}

/// The default rating: the first of the "rating sources" option that the movie has.
pub(crate) fn main_rating<'a>(
    movie: &'a MovieNfo,
    options: &MovieNfoOptions,
) -> Option<&'a Rating> {
    options.rating_sources.iter().find_map(|source| {
        let wanted = rating_name(source.trim());
        external_ratings(movie).find(|r| rating_name(&r.id) == wanted)
    })
}

pub(crate) fn rating_element(rating: &Rating, default: bool) -> Element {
    Element::new("rating")
        .attr("default", if default { "true" } else { "false" })
        .attr("max", rating.max_value.to_string())
        .attr("name", rating_name(&rating.id))
        .child(Element::text("value", format::fixed(rating.value, 1)))
        .child(Element::text("votes", rating.votes.to_string()))
}

pub(crate) fn user_rating(movie: &MovieNfo) -> String {
    movie
        .ratings
        .get(cv_domain::rating_key::USER)
        .map_or_else(|| "0".to_owned(), |r| format::user_rating(r.on_ten_scale()))
}

// ---- set, outline, artwork ---------------------------------------------------------------

/// `<set><name/><overview/></set>`; Emby adds the `tmdbcolid` attribute when the set has an ID.
fn movie_set(set: Option<&MovieSetRef>, with_collection_id: bool) -> Element {
    match set {
        Some(set) => {
            let mut element = Element::new("set");
            if with_collection_id && let Some(id) = set.tmdb_id {
                element = element.attr("tmdbcolid", id.to_string());
            }
            element
                .child(Element::text("name", &set.title))
                .child(Element::text("overview", &set.overview))
        }
        None => Element::new("set"),
    }
}

/// The other `<set>` entries of the old NFO (Emby): named, and not the movie's own set. Each one
/// is written with its TMDB collection ID in `tmdbcolid`, never as the text of the set object.
fn other_sets(movie: &MovieNfo) -> impl Iterator<Item = &MovieSetRef> {
    let own = movie.movie_set.as_ref().map(|s| s.title.trim());
    movie.other_sets.iter().filter(move |set| {
        let title = set.title.trim();
        !title.is_empty() && own.is_none_or(|own| !own.eq_ignore_ascii_case(title))
    })
}

/// The outline: the movie's own, else one created from the plot, else the one of the old file.
pub(crate) fn outline(movie: &MovieNfo, options: &MovieNfoOptions) -> Option<String> {
    if !movie.outline.trim().is_empty() {
        return Some(movie.outline.clone());
    }
    if options.create_outline && !movie.plot.trim().is_empty() {
        return Some(outline_from_plot(
            &movie.plot,
            options.outline_first_sentence,
        ));
    }
    Some(movie.carried.outline.clone()).filter(|o| !o.trim().is_empty())
}

/// Minimum length of a first sentence that is used as an outline.
const MIN_OUTLINE_CHARS: usize = 20;

fn outline_from_plot(plot: &str, first_sentence: bool) -> String {
    if first_sentence {
        let sentence = first_sentence_of(plot);
        if sentence.chars().count() >= MIN_OUTLINE_CHARS {
            return sentence.to_owned();
        }
    }
    plot.to_owned()
}

/// The text up to the first `.`, `!` or `?` that ends a sentence (is followed by white space or
/// the end of the text), without that mark. The whole text when there is none.
fn first_sentence_of(text: &str) -> &str {
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(c, '.' | '!' | '?') && chars.peek().is_none_or(|(_, next)| next.is_whitespace())
        {
            return text[..i].trim();
        }
    }
    text.trim()
}

pub(crate) fn artwork(movie: &MovieNfo, root: &mut Element) {
    let art = &movie.artwork;
    for (aspect, url) in [
        ("poster", &art.poster),
        ("banner", &art.banner),
        ("clearart", &art.clearart),
        ("clearlogo", &art.clearlogo),
        ("discart", &art.disc),
        ("landscape", &art.thumb),
        ("keyart", &art.keyart),
        ("squareart", &art.squareart),
        ("logo", &art.logo),
    ] {
        if !url.trim().is_empty() {
            root.push(Element::text("thumb", url.trim()).attr("aspect", aspect));
        }
    }
    let fanart: Vec<&str> = std::iter::once(art.fanart.as_str())
        .chain(art.extra_fanarts.iter().map(String::as_str))
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .collect();
    if !fanart.is_empty() {
        let mut element = Element::new("fanart");
        for url in fanart {
            element.push(Element::text("thumb", url));
        }
        root.push(element);
    }
}

// ---- ids -------------------------------------------------------------------------------------

pub(crate) fn unique_ids(movie: &MovieNfo, root: &mut Element) {
    let mut ids: Vec<(&str, &str)> = movie.ids.iter().collect();
    if ids.is_empty() {
        return;
    }
    format::stable_map_order(&mut ids, |pair| pair.0.to_owned());
    let default = ["imdb", "tvdb", "tmdb"]
        .into_iter()
        .find(|key| ids.iter().any(|(k, _)| k == key))
        .or_else(|| ids.first().map(|(k, _)| *k));
    for (key, value) in ids {
        root.push(
            Element::text("uniqueid", value)
                .attr(
                    "default",
                    if Some(key) == default {
                        "true"
                    } else {
                        "false"
                    },
                )
                .attr("type", key),
        );
    }
}

// ---- people ----------------------------------------------------------------------------------

pub(crate) fn credits_of(movie: &MovieNfo, kind: CreditKind) -> impl Iterator<Item = &Credit> {
    movie.credits.iter().filter(move |c| c.kind == kind)
}

/// `(attribute or element name, id)` for the IDs a person can carry, in the order they are written
/// as child elements.
pub(crate) fn person_ids(person: &Person) -> [(&'static str, &str); 3] {
    [
        ("tmdbid", person.ids.get("tmdb").unwrap_or_default()),
        ("imdbid", person.ids.get("imdb").unwrap_or_default()),
        ("tvdbid", person.ids.get("tvdb").unwrap_or_default()),
    ]
}

/// `<credits imdbid="..">Name</credits>` and `<director tmdbid="..">Name</director>`.
pub(crate) fn named_person(element: &str, credit: &Credit) -> Element {
    let mut e = Element::text(element, &credit.person.name);
    for (name, value) in person_ids(&credit.person) {
        if !value.is_empty() {
            e = e.attr(name, value);
        }
    }
    e
}

pub(crate) fn picture_children(element: &mut Element, person: &Person, options: &MovieNfoOptions) {
    if options.artwork_urls && !person.thumb_url.trim().is_empty() {
        element.push(Element::text("thumb", person.thumb_url.trim()));
    }
    if !person.profile_url.trim().is_empty() {
        element.push(Element::text("profile", person.profile_url.trim()));
    }
}

pub(crate) fn actor(
    name: &str,
    credit: &Credit,
    options: &MovieNfoOptions,
    with_ids: bool,
) -> Element {
    let mut e = Element::new(name).child(Element::text("name", &credit.person.name));
    // A person without a role gets no `<role>` at all: some media players
    // fail on an empty `<role/>`.
    if !credit.role.trim().is_empty() {
        e.push(Element::text("role", &credit.role));
    }
    picture_children(&mut e, &credit.person, options);
    if with_ids {
        for (id_name, value) in person_ids(&credit.person) {
            if !value.is_empty() {
                e.push(Element::text(id_name, value));
            }
        }
    }
    e
}

pub(crate) fn producer(credit: &Credit, options: &MovieNfoOptions) -> Element {
    let mut e = actor("producer", credit, options, false);
    for (name, value) in person_ids(&credit.person) {
        if !value.is_empty() {
            e = e.attr(name, value);
        }
    }
    e
}

pub(crate) fn crew_type(kind: CreditKind) -> &'static str {
    match kind {
        CreditKind::Actor => "ACTOR",
        CreditKind::Director => "DIRECTOR",
        CreditKind::Writer => "WRITER",
        CreditKind::Producer => "PRODUCER",
        CreditKind::Guest => "GUEST",
        CreditKind::Composer => "COMPOSER",
        CreditKind::Editor => "EDITOR",
        CreditKind::Camera => "CAMERA",
        CreditKind::Other => "OTHER",
    }
}

pub(crate) fn crew(credit: &Credit, options: &MovieNfoOptions) -> Element {
    let mut role = Element::text("role", crew_type(credit.kind));
    if !credit.role.trim().is_empty() {
        role = role.attr("subrole", credit.role.trim());
    }
    let mut e = Element::new("crew")
        .child(Element::text("name", &credit.person.name))
        .child(role);
    picture_children(&mut e, &credit.person, options);
    for (name, value) in person_ids(&credit.person) {
        if !value.is_empty() {
            e.push(Element::text(name, value));
        }
    }
    e
}

// ---- trailer, languages, dates -----------------------------------------------------------

pub(crate) fn trailer(trailers: &[Trailer]) -> String {
    trailers
        .iter()
        .find(|t| t.in_nfo && !t.url.trim().is_empty())
        .map(|t| trailer_url(t.url.trim()))
        .unwrap_or_default()
}

/// A web trailer as the Kodi `<trailer>` text: a YouTube link becomes the YouTube plug-in URL, a
/// link on one of the trailer sites the HD-Trailers plug-in plays becomes that plug-in URL, and
/// anything else (including text that is not a web link) is written as it is.
pub(crate) fn trailer_url(url: &str) -> String {
    if !url.starts_with("http") {
        return url.to_owned();
    }
    match youtube_id(url) {
        Some(id) => format!("plugin://plugin.video.youtube/play/?video_id={id}"),
        None => hdtrailers_plugin_url(url).unwrap_or_else(|| url.to_owned()),
    }
}

/// The older XBMC form of [`trailer_url`]: the YouTube plug-in with `action=play_video`, the same
/// HD-Trailers plug-in URL, and anything else as it is.
pub(crate) fn xbmc_trailer_url(url: &str) -> String {
    match youtube_id(url) {
        Some(id) => format!("plugin://plugin.video.youtube/?action=play_video&videoid={id}"),
        None => hdtrailers_plugin_url(url).unwrap_or_else(|| url.to_owned()),
    }
}

/// The sites whose trailer links the HD-Trailers plug-in plays. A `.` stands for any character.
const HDTRAILERS_SITES: [&str; 8] = [
    "apple.com",
    "yahoo-redir",
    "yahoo.com",
    "youtube.com",
    "moviefone.com",
    "ign.com",
    "hd-trailers.net",
    "aol.com",
];

/// `plugin://plugin.video.hdtrailers_net/video/<site>/<url-encoded link>` for an http(s) link that
/// contains one of the [`HDTRAILERS_SITES`]. The site is the last one found in the link; at one
/// position the earlier entry of the list wins.
fn hdtrailers_plugin_url(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    if rest.contains(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}']) {
        return None;
    }
    let chars: Vec<char> = rest.chars().collect();
    let site = (0..chars.len()).rev().find_map(|start| {
        HDTRAILERS_SITES.iter().find_map(|site| {
            let window = chars.get(start..start + site.chars().count())?;
            site.chars()
                .zip(window)
                .all(|(want, got)| want == '.' || want == *got)
                .then(|| window.iter().collect::<String>())
        })
    })?;
    Some(format!(
        "plugin://plugin.video.hdtrailers_net/video/{site}/{}",
        form_encode(url)
    ))
}

/// Form-style URL encoding: ASCII letters, digits and `.-*_` stay, a space becomes `+`, every
/// other byte of the UTF-8 text becomes `%XX` in upper case.
pub(crate) fn form_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub(crate) fn youtube_id(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_lowercase();
    let host = host.trim_start_matches("www.").trim_start_matches("m.");
    let id_of = |text: &str| -> Option<String> {
        let id: String = text
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        (!id.is_empty()).then_some(id)
    };
    match host {
        "youtu.be" => id_of(path),
        "youtube.com" | "youtube-nocookie.com" => {
            if let Some((route, query)) = path.split_once('?')
                && route == "watch"
            {
                return query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("v="))
                    .and_then(id_of);
            }
            ["embed/", "v/", "shorts/"]
                .into_iter()
                .find_map(|prefix| path.strip_prefix(prefix))
                .and_then(id_of)
        }
        _ => None,
    }
}

pub(crate) fn languages(movie: &MovieNfo, options: &MovieNfoOptions) -> String {
    languages_list(movie, options).join(", ")
}

/// The spoken languages as names in the NFO language.
pub(crate) fn languages_list(movie: &MovieNfo, options: &MovieNfoOptions) -> Vec<String> {
    non_blank(&movie.spoken_languages)
        .map(|l| locale::language_name(l, &options.language))
        .collect()
}

/// An artwork URL as written: trimmed; empty when there is none.
pub(crate) fn artwork_url_text(url: &str) -> &str {
    url.trim()
}

/// The play count: the higher of the stored and the carried one, at least 1 for a watched movie,
/// 0 for an unwatched one.
pub(crate) fn play_count(movie: &MovieNfo) -> u32 {
    if movie.watched {
        movie.play_count.max(movie.carried.play_count).max(1)
    } else {
        0
    }
}

/// The later of the stored and the carried last-played time; only a watched movie has one.
pub(crate) fn last_played(movie: &MovieNfo) -> Option<String> {
    if !movie.watched {
        return None;
    }
    [&movie.last_played, &movie.carried.last_played]
        .into_iter()
        .flatten()
        .filter(|t| !t.is_empty())
        .max()
        .cloned()
}

pub(crate) fn date_added(movie: &MovieNfo, options: &MovieNfoOptions) -> Option<String> {
    let fallback = || movie.date_added.clone();
    match options.date_added_field {
        DateAddedField::DateAdded => fallback(),
        DateAddedField::FileCreationDate => movie.video.created.clone().or_else(fallback),
        DateAddedField::FileLastModifiedDate => movie.video.modified.clone().or_else(fallback),
        DateAddedField::ReleaseDate => movie
            .release_date
            .map(|d| format!("{d} 00:00:00"))
            .or_else(fallback),
    }
}

/// The text Kodi shows for an edition (`videoassettitle`).
pub(crate) fn edition_title(edition: &Edition) -> String {
    match edition {
        Edition::None => String::new(),
        Edition::DirectorsCut => "Director's Cut".to_owned(),
        Edition::ExtendedEdition => "Extended Edition".to_owned(),
        Edition::TheatricalEdition => "Theatrical Edition".to_owned(),
        Edition::Unrated => "Unrated".to_owned(),
        Edition::Uncut => "Uncut".to_owned(),
        Edition::Imax => "IMAX".to_owned(),
        Edition::Remastered => "Remastered".to_owned(),
        Edition::CollectorsEdition => "Collector's Edition".to_owned(),
        Edition::UltimateEdition => "Ultimate Edition".to_owned(),
        Edition::FinalCut => "Final Cut".to_owned(),
        Edition::SpecialEdition => "Special Edition".to_owned(),
        Edition::CriterionCollection => "Criterion Collection".to_owned(),
        Edition::OpenMatte => "Open Matte".to_owned(),
        Edition::Other(text) => text.clone(),
    }
}

// ---- fileinfo --------------------------------------------------------------------------------

pub(crate) fn fileinfo(movie: &MovieNfo, options: &MovieNfoOptions) -> Element {
    let external_audio: &[AudioStream] = if options.include_external_audio {
        &movie.external_audio
    } else {
        &[]
    };
    let duration = if movie.is_disc {
        disc_duration(movie, options)
    } else {
        movie.video.duration_secs
    };
    fileinfo_of(
        &movie.video,
        duration,
        external_audio,
        &movie.external_subtitles,
        options.cv_extensions,
    )
}

/// The duration of a disc structure. There is no single file to measure, so the basic layout
/// writes the scraped runtime (when it is at least 10 seconds), else 0. With CineVault's additions the
/// measured length of the disc's main title wins when the media info has it.
fn disc_duration(movie: &MovieNfo, options: &MovieNfoOptions) -> u32 {
    let scraped = movie.runtime_minutes.saturating_mul(60);
    if options.cv_extensions && movie.video.duration_secs > 0 {
        movie.video.duration_secs
    } else if scraped >= 10 {
        scraped
    } else {
        0
    }
}

/// `<fileinfo>` of a video: the video, its audio streams (and `external_audio`) and the
/// subtitles of the video and of the files next to it. `duration` is written as the duration.
/// `extended` adds CineVault's stream details after the standard Kodi elements of each stream
/// (see [`extended_video`], [`extended_audio`] and [`extended_subtitle`]).
pub(crate) fn fileinfo_of(
    video: &crate::streams::VideoInfo,
    duration: u32,
    external_audio: &[AudioStream],
    external_subtitles: &[cv_domain::SubtitleStream],
    extended: bool,
) -> Element {
    let ratio = if video.aspect_ratio > 0.0 {
        streams::common_aspect_ratio(video.aspect_ratio)
    } else if video.width > 0 && video.height > 0 {
        streams::common_aspect_ratio(video.width as f32 / video.height as f32)
    } else {
        0.0
    };
    let mut v = Element::new("video")
        .child(Element::text(
            "codec",
            streams::video_codec_token(&video.codec),
        ))
        .child(Element::text("aspect", format::fixed(ratio, 2)))
        .child(Element::text("width", video.width.to_string()))
        .child(Element::text("height", video.height.to_string()))
        .child(Element::text(
            "resolution",
            streams::resolution_token(video.width, video.height),
        ));
    if let Some(hdr) = streams::hdr_type_token(&video.hdr_formats) {
        v.push(Element::text("hdrtype", hdr));
    }
    if let Some(mode) = streams::stereo_mode_token(&video.stereo_format) {
        v.push(Element::text("stereomode", mode));
    }
    v.push(Element::text("durationinseconds", duration.to_string()));
    if extended {
        extended_video(video, &mut v);
    }

    let mut details = Element::new("streamdetails").child(v);
    for audio in video.audio.iter().chain(external_audio) {
        let mut a = Element::new("audio")
            .child(Element::text(
                "codec",
                streams::audio_codec_token(&audio.codec),
            ))
            .child(Element::text(
                "language",
                locale::stream_language_code(&audio.language),
            ))
            .child(Element::text("channels", audio.channels.to_string()));
        if extended {
            extended_audio(audio, &mut a);
        }
        details.push(a);
    }
    let embedded = video.subtitles.iter().map(|s| (s, false));
    let sidecars = external_subtitles.iter().map(|s| (s, true));
    for (subtitle, external) in embedded.chain(sidecars) {
        let mut s = Element::new("subtitle").child(Element::text(
            "language",
            locale::stream_language_code(&subtitle.language),
        ));
        if extended {
            extended_subtitle(subtitle, external, &mut s);
        }
        details.push(s);
    }
    Element::new("fileinfo").child(details)
}

/// A number element, left out when the value is `0`.
fn push_number(element: &mut Element, name: &str, value: u64) {
    if value > 0 {
        element.push(Element::text(name, value.to_string()));
    }
}

/// A text element, left out when the text is blank.
fn push_text(element: &mut Element, name: &str, value: &str) {
    if !value.trim().is_empty() {
        element.push(Element::text(name, value.trim()));
    }
}

/// A frame rate with at most three decimals and no trailing zeros: `23.976`, `24`, `29.97`.
pub(crate) fn frame_rate_text(rate: f64) -> String {
    let text = format!("{rate:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// CineVault's video details after `<durationinseconds>`: the container, the video and overall
/// bitrates in kbit/s, the frame rate, the bit depth, every HDR format (`<hdrtype>` holds one)
/// and the size in bytes. Unknown values are left out.
fn extended_video(video: &crate::streams::VideoInfo, v: &mut Element) {
    push_text(v, "container", &video.container);
    push_number(v, "bitrate", u64::from(video.video_bitrate_kbps));
    push_number(v, "overallbitrate", u64::from(video.overall_bitrate_kbps));
    if video.frame_rate > 0.0 && video.frame_rate.is_finite() {
        v.push(Element::text(
            "framerate",
            frame_rate_text(video.frame_rate),
        ));
    }
    push_number(v, "bitdepth", u64::from(video.bit_depth));
    let formats: Vec<&str> = video
        .hdr_formats
        .iter()
        .map(|f| f.trim())
        .filter(|f| !f.is_empty())
        .collect();
    if !formats.is_empty() {
        v.push(Element::text("hdrformats", formats.join(", ")));
    }
    push_number(v, "filesize", video.size_bytes);
}

/// The flags of a stream as `<name>true</name>` elements, in a fixed order.
fn push_flags(element: &mut Element, flags: &std::collections::BTreeSet<cv_domain::StreamFlag>) {
    for flag in flags {
        element.push(Element::text(stream_flag_name(*flag), "true"));
    }
}

/// The element name of a stream flag.
pub(crate) fn stream_flag_name(flag: cv_domain::StreamFlag) -> &'static str {
    use cv_domain::StreamFlag as F;
    match flag {
        F::Default => "default",
        F::Dub => "dub",
        F::Original => "original",
        F::Comment => "commentary",
        F::Lyrics => "lyrics",
        F::Karaoke => "karaoke",
        F::Forced => "forced",
        F::HearingImpaired => "hearingimpaired",
        F::VisualImpaired => "visualimpaired",
    }
}

/// CineVault's audio details after `<channels>`: the stream title, the bitrate in kbit/s, the
/// bit depth, the sample rate in Hz and the stream flags.
fn extended_audio(audio: &AudioStream, a: &mut Element) {
    push_text(a, "title", &audio.title);
    push_number(a, "bitrate", u64::from(audio.bitrate_kbps));
    push_number(a, "bitdepth", u64::from(audio.bit_depth));
    push_number(a, "samplerate", u64::from(audio.sample_rate_hz));
    push_flags(a, &audio.flags);
}

/// CineVault's subtitle details after `<language>`: the format, the title, the flags (forced,
/// hearing impaired, ...) and whether it is a file next to the video.
fn extended_subtitle(subtitle: &cv_domain::SubtitleStream, external: bool, s: &mut Element) {
    push_text(s, "codec", &subtitle.codec);
    push_text(s, "title", &subtitle.title);
    push_flags(s, &subtitle.flags);
    if external {
        s.push(Element::text("external", "true"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sentence_stops_at_a_sentence_end() {
        assert_eq!(first_sentence_of("One two. Three."), "One two");
        assert_eq!(first_sentence_of("Is it? Yes."), "Is it");
        assert_eq!(
            first_sentence_of("Version 1.5 is out. Next"),
            "Version 1.5 is out"
        );
        assert_eq!(first_sentence_of("No end"), "No end");
        assert_eq!(first_sentence_of("Ends here."), "Ends here");
    }

    #[test]
    fn a_short_first_sentence_gives_the_whole_plot() {
        assert_eq!(
            outline_from_plot("Short one. And more text here.", true),
            "Short one. And more text here."
        );
        assert_eq!(
            outline_from_plot("A sentence that is long enough. More.", true),
            "A sentence that is long enough"
        );
        assert_eq!(
            outline_from_plot("A sentence that is long enough. More.", false),
            "A sentence that is long enough. More."
        );
    }

    #[test]
    fn youtube_links_become_plugin_urls() {
        let f = |url: &str| trailer_url(url);
        let want = "plugin://plugin.video.youtube/play/?video_id=abc123XYZ_-";
        assert_eq!(f("https://www.youtube.com/watch?v=abc123XYZ_-"), want);
        assert_eq!(
            f("https://www.youtube.com/watch?feature=x&v=abc123XYZ_-&t=3"),
            want
        );
        assert_eq!(f("https://youtu.be/abc123XYZ_-?t=3"), want);
        assert_eq!(f("https://www.youtube.com/embed/abc123XYZ_-"), want);
        assert_eq!(
            f("https://example.test/trailer.mp4"),
            "https://example.test/trailer.mp4"
        );
    }

    #[test]
    fn trailer_site_links_become_hdtrailers_plugin_urls() {
        let hd = "plugin://plugin.video.hdtrailers_net/video/";
        assert_eq!(
            trailer_url("https://www.hd-trailers.net/movie/heat/"),
            format!("{hd}hd-trailers.net/https%3A%2F%2Fwww.hd-trailers.net%2Fmovie%2Fheat%2F")
        );
        assert_eq!(
            trailer_url("http://trailers.apple.com/movies/wb/heat/heat-tlr1_720p.mov"),
            format!(
                "{hd}apple.com/http%3A%2F%2Ftrailers.apple.com%2Fmovies%2Fwb%2Fheat%2Fheat-tlr1_720p.mov"
            )
        );
        assert_eq!(
            trailer_url("https://www.moviefone.com/a b/ü?x=1&y=*"),
            format!(
                "{hd}moviefone.com/https%3A%2F%2Fwww.moviefone.com%2Fa+b%2F%C3%BC%3Fx%3D1%26y%3D*"
            )
        );
        // a YouTube link that is not a video link still goes through the plug-in
        assert_eq!(
            trailer_url("https://www.youtube.com/user/studio"),
            format!("{hd}youtube.com/https%3A%2F%2Fwww.youtube.com%2Fuser%2Fstudio")
        );
    }

    #[test]
    fn the_last_trailer_site_in_the_link_names_the_plugin_path() {
        let hd = "plugin://plugin.video.hdtrailers_net/video/";
        // the site name is the last one found; the earlier list entry wins at one position
        assert_eq!(
            trailer_url("https://yahoo-redir.test/r?u=trailers.apple.com/x"),
            format!(
                "{hd}apple.com/https%3A%2F%2Fyahoo-redir.test%2Fr%3Fu%3Dtrailers.apple.com%2Fx"
            )
        );
        assert!(trailer_url("https://x.test/yahoo.com").contains("/video/yahoo.com/"));
        // `.` in a site name matches any character
        assert!(trailer_url("https://x.test/appleXcom").contains("/video/appleXcom/"));
    }

    #[test]
    fn other_trailer_text_is_written_as_it_is() {
        for text in [
            "https://cdn.example.test/t.mp4",
            "ftp://trailers.apple.com/t.mov",
            "trailers.apple.com/t.mov",
            "/media/trailers/t.mp4",
            "plugin://plugin.video.youtube/play/?video_id=abc",
        ] {
            assert_eq!(trailer_url(text), text);
            assert_eq!(xbmc_trailer_url(text), text);
        }
    }

    #[test]
    fn xbmc_trailers_use_the_older_youtube_url_and_the_same_hdtrailers_url() {
        assert_eq!(
            xbmc_trailer_url("https://www.youtube.com/watch?v=abc123"),
            "plugin://plugin.video.youtube/?action=play_video&videoid=abc123"
        );
        assert_eq!(
            xbmc_trailer_url("https://www.hd-trailers.net/movie/heat/"),
            trailer_url("https://www.hd-trailers.net/movie/heat/")
        );
    }
}
