//! The XBMC and MediaPortal movie NFO writers.
//!
//! Both use the older flat layout: one `<rating>` (on a ten scale, one decimal) followed by
//! `<userrating>` and `<votes>`, a plain-text `<set>`, one poster `<thumb>` and one `<fanart>`. The
//! element order is the one in `tests/compat/expected` (`movie-xbmc-*`, `movie-mp-*`):
//!
//! ```text
//! XBMC:        title originaltitle sorttitle epbookmark year rating userrating votes [top250] set
//!              plot [outline] tagline runtime [thumb] [fanart] mpaa certification id tmdbid
//!              uniqueid* country* status code premiered watched playcount [lastplayed] genre*
//!              studio* credits* director* tag* actor* producer* [trailer] languages showlink*
//!              [dateadded] [lockdata] [fileinfo] <unmanaged elements> <comment> <private tags>
//! MediaPortal: title originaltitle sorttitle year rating userrating votes set plot [outline]
//!              tagline runtime [thumb] [fanart] [mpaa] [certification] [imdb] id tmdbid uniqueid*
//!              country premiered watched playcount [lastplayed] genre* studio credits director
//!              tag* actor* producer* [trailer] language(s) showlink* [dateadded] [lockdata]
//!              [fileinfo] <unmanaged elements> <comment> <private tags>
//! ```
//!
//! Differences from the Kodi writer:
//!
//! * The XBMC `<fileinfo>` is plain: codec and language texts are written as stored, there is no
//!   `<resolution>` or `<hdrtype>`, the aspect ratio is the common ratio printed in its shortest
//!   form (`2.4`, `1.78`, `0.0`) and `<stereomode/>` is always present. No edition elements.
//!   Video codecs keep their stored name, except that HEVC is upper case.
//! * The XBMC trailer is the older plug-in URL (`?action=play_video&videoid=ID`).
//! * MediaPortal writes the Kodi `<fileinfo>`, the trailer URL as it is, `credits` and `director`
//!   as one comma-joined element, and country and studio as one text.
//!
//! The three MediaPortal styles differ in a few elements:
//!
//! | Style | certification | genres | country, studio | languages |
//! |---|---|---|---|---|
//! | legacy (`MP`) | `<certification>` | `<genres><genre/>..</genres>` | the stored text | `<languages>` joined with `\|` |
//! | Moving Pictures (`MP_MP`) | `<mpaa>` | `<genre>` x n | joined with `, ` | `<language>` joined with `, ` |
//! | MyVideo (`MP_MV`) | `<mpaa>`, `<certification>`, `<imdb>` | `<genre>` x n | joined with ` / ` | `<language>` joined with `\|` |

use cv_domain::{CreditKind, Rating};

use crate::format;
use crate::kodi_movie::{
    actor, artwork_url_text, credits_of, date_added, fileinfo, finish, languages_list, last_played,
    main_rating, non_blank, number_or_empty, outline, play_count, private_tags, producer,
    unique_ids, user_rating, xbmc_trailer_url,
};
use crate::movie::{MovieNfo, MovieNfoOptions};
use crate::streams;
use crate::xml::{Document, Element};

/// The MediaPortal NFO styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaPortalStyle {
    /// The legacy MediaPortal NFO (setting `MP`).
    Legacy,
    /// Moving Pictures (setting `MP_MP`).
    MovingPictures,
    /// MyVideo (setting `MP_MV`).
    MyVideo,
}

// ---- XBMC ------------------------------------------------------------------------------------

/// Builds the XBMC NFO document of a movie.
pub(crate) fn xbmc_document(movie: &MovieNfo, options: &MovieNfoOptions) -> Document {
    let mut root = Element::new("movie");
    let text = |name: &str, value: &str| Element::text(name, value);

    head(movie, options, &mut root, Some(&movie.carried.epbookmark));
    if movie.top250 > 0 {
        root.push(text("top250", &movie.top250.to_string()));
    }
    set_and_text(movie, options, &mut root, true);

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
    root.push(text("status", &movie.carried.status));
    root.push(text("code", &movie.carried.code));
    watch_state(movie, &mut root);
    for genre in &movie.genres {
        root.push(text(
            "genre",
            &crate::locale::genre_name(genre, &options.language),
        ));
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
    for credit in credits_of(movie, CreditKind::Writer) {
        root.push(crate::kodi_movie::named_person("credits", credit));
    }
    for credit in credits_of(movie, CreditKind::Director) {
        root.push(crate::kodi_movie::named_person("director", credit));
    }
    for tag in non_blank(&movie.tags) {
        root.push(text("tag", tag));
    }
    people(movie, options, &mut root);
    if options.trailer {
        root.push(text("trailer", &xbmc_trailer(movie)));
    }
    root.push(text(
        "languages",
        &languages_list(movie, options).join(", "),
    ));
    for link in non_blank(&movie.show_links) {
        root.push(text("showlink", link));
    }
    tail(movie, options, &mut root, xbmc_fileinfo);
    finish(root, options, "XBMC")
}

/// The XBMC trailer: a YouTube link becomes the older plug-in URL, a link on a trailer site the
/// HD-Trailers plug-in plays becomes that plug-in URL, any other text stays.
fn xbmc_trailer(movie: &MovieNfo) -> String {
    let url = raw_trailer(movie);
    if url.starts_with("http") {
        xbmc_trailer_url(&url)
    } else {
        url
    }
}

fn xbmc_fileinfo(movie: &MovieNfo, options: &MovieNfoOptions) -> Element {
    let external_audio: &[cv_domain::AudioStream] = if options.include_external_audio {
        &movie.external_audio
    } else {
        &[]
    };
    xbmc_fileinfo_of(
        &movie.video,
        streams_duration(movie),
        external_audio,
        &movie.external_subtitles,
    )
}

/// The plain XBMC `<fileinfo>` of a video; `duration` is written as the duration in seconds.
pub(crate) fn xbmc_fileinfo_of(
    video: &streams::VideoInfo,
    duration: u32,
    external_audio: &[cv_domain::AudioStream],
    external_subtitles: &[cv_domain::SubtitleStream],
) -> Element {
    let ratio = display_ratio_of(video);
    let mut v = Element::new("video")
        .child(Element::text("codec", xbmc_video_codec(&video.codec)))
        .child(Element::text("aspect", short_float(ratio)))
        .child(Element::text("width", video.width.to_string()))
        .child(Element::text("height", video.height.to_string()))
        .child(Element::text("durationinseconds", duration.to_string()));
    v.push(Element::text(
        "stereomode",
        streams::stereo_mode_token(&video.stereo_format).unwrap_or_default(),
    ));

    let mut details = Element::new("streamdetails").child(v);
    for audio in video.audio.iter().chain(external_audio) {
        details.push(
            Element::new("audio")
                .child(Element::text("codec", audio.codec.trim()))
                .child(Element::text("language", audio.language.trim()))
                .child(Element::text("channels", audio.channels.to_string())),
        );
    }
    for subtitle in video.subtitles.iter().chain(external_subtitles) {
        details.push(
            Element::new("subtitle").child(Element::text("language", subtitle.language.trim())),
        );
    }
    Element::new("fileinfo").child(details)
}

/// The video codec as XBMC spells it: the stored name, except that HEVC is upper case.
fn xbmc_video_codec(codec: &str) -> String {
    let codec = codec.trim();
    match codec.to_ascii_lowercase().as_str() {
        "hevc" | "h265" | "h.265" => "HEVC".to_owned(),
        _ => codec.to_owned(),
    }
}

/// The shortest text of a single-precision number with at least one decimal: `2.4`, `1.78`,
/// `0.0`.
fn short_float(value: f32) -> String {
    format!("{value:?}")
}

fn display_ratio_of(video: &streams::VideoInfo) -> f32 {
    if video.aspect_ratio > 0.0 {
        streams::common_aspect_ratio(video.aspect_ratio)
    } else if video.width > 0 && video.height > 0 {
        streams::common_aspect_ratio(video.width as f32 / video.height as f32)
    } else {
        0.0
    }
}

/// A disc structure has no single file duration to report.
fn streams_duration(movie: &MovieNfo) -> u32 {
    if movie.is_disc {
        0
    } else {
        movie.video.duration_secs
    }
}

// ---- MediaPortal -----------------------------------------------------------------------------

/// Builds the MediaPortal NFO document of a movie in one of the three styles.
pub(crate) fn media_portal_document(
    style: MediaPortalStyle,
    movie: &MovieNfo,
    options: &MovieNfoOptions,
) -> Document {
    let mut root = Element::new("movie");
    let text = |name: &str, value: &str| Element::text(name, value);

    head(movie, options, &mut root, None);
    set_and_text(
        movie,
        options,
        &mut root,
        style == MediaPortalStyle::MovingPictures,
    );

    let certification = crate::certification::format_certification(
        &movie.certification.0,
        options.certification_style,
    );
    if style != MediaPortalStyle::Legacy {
        root.push(text("mpaa", &certification));
    }
    if style != MediaPortalStyle::MovingPictures {
        root.push(text("certification", &certification));
    }
    if style == MediaPortalStyle::MyVideo {
        root.push(text("imdb", movie.ids.get("imdb").unwrap_or_default()));
    }
    root.push(text("id", movie.ids.get("imdb").unwrap_or_default()));
    root.push(text("tmdbid", movie.ids.get("tmdb").unwrap_or_default()));
    unique_ids(movie, &mut root);
    root.push(text("country", &country_text(style, movie)));
    watch_state(movie, &mut root);

    let genres: Vec<String> = movie
        .genres
        .iter()
        .map(|g| crate::locale::genre_name(g, &options.language))
        .collect();
    if style == MediaPortalStyle::Legacy {
        let mut element = Element::new("genres");
        for genre in &genres {
            element.push(text("genre", genre));
        }
        root.push(element);
    } else {
        for genre in &genres {
            root.push(text("genre", genre));
        }
    }
    root.push(text("studio", &studio_text(style, movie, options)));
    root.push(text("credits", &names_of(movie, CreditKind::Writer)));
    root.push(text("director", &names_of(movie, CreditKind::Director)));
    for tag in non_blank(&movie.tags) {
        root.push(text("tag", tag));
    }
    people(movie, options, &mut root);
    if options.trailer {
        root.push(text("trailer", &raw_trailer(movie)));
    }
    let languages = languages_list(movie, options);
    match style {
        MediaPortalStyle::Legacy => root.push(text("languages", &languages.join("|"))),
        MediaPortalStyle::MovingPictures => root.push(text("language", &languages.join(", "))),
        MediaPortalStyle::MyVideo => root.push(text("language", &languages.join("|"))),
    }
    for link in non_blank(&movie.show_links) {
        root.push(text("showlink", link));
    }
    tail(movie, options, &mut root, fileinfo);
    finish(root, options, "MediaPortal")
}

fn separator(style: MediaPortalStyle) -> &'static str {
    match style {
        MediaPortalStyle::MyVideo => " / ",
        _ => ", ",
    }
}

/// Legacy writes the country text as stored; the other styles join the list.
fn country_text(style: MediaPortalStyle, movie: &MovieNfo) -> String {
    if style == MediaPortalStyle::Legacy && !movie.country_text.trim().is_empty() {
        return movie.country_text.trim().to_owned();
    }
    non_blank(&movie.countries)
        .collect::<Vec<_>>()
        .join(separator(style))
}

/// Legacy writes the studio text as stored; the other styles join the list. With "write only the
/// first studio" only the first studio of the list is written.
fn studio_text(style: MediaPortalStyle, movie: &MovieNfo, options: &MovieNfoOptions) -> String {
    if style == MediaPortalStyle::Legacy
        && !options.single_studio
        && !movie.studio_text.trim().is_empty()
    {
        return movie.studio_text.trim().to_owned();
    }
    let studios = non_blank(&movie.studios);
    let studios: Vec<&str> = if options.single_studio {
        studios.take(1).collect()
    } else {
        studios.collect()
    };
    studios.join(separator(style))
}

/// The names of all credits of one kind, joined with a comma.
fn names_of(movie: &MovieNfo, kind: CreditKind) -> String {
    credits_of(movie, kind)
        .map(|c| c.person.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

// ---- shared pieces ---------------------------------------------------------------------------

/// `title originaltitle sorttitle [epbookmark] year rating userrating votes`.
fn head(movie: &MovieNfo, options: &MovieNfoOptions, root: &mut Element, bookmark: Option<&str>) {
    let text = |name: &str, value: &str| Element::text(name, value);
    root.push(text("title", &movie.title));
    root.push(text("originaltitle", &movie.original_title));
    root.push(text("sorttitle", &movie.sort_title));
    if let Some(bookmark) = bookmark {
        root.push(text("epbookmark", bookmark));
    }
    root.push(text("year", &number_or_empty(movie.year)));
    let (value, votes) = match main_rating(movie, options) {
        Some(rating) => rating_text(rating),
        None => ("0.0".to_owned(), "0".to_owned()),
    };
    root.push(text("rating", &value));
    root.push(text("userrating", &user_rating(movie)));
    root.push(text("votes", &votes));
}

/// The rating on a ten scale with one decimal, and its votes.
fn rating_text(rating: &Rating) -> (String, String) {
    (
        format::fixed(rating.on_ten_scale(), 1),
        rating.votes.to_string(),
    )
}

/// `set plot [outline] tagline runtime [thumb] [fanart]`; the fanart URL is the text of the element
/// when `plain_fanart` is set and a nested `<thumb>` otherwise.
fn set_and_text(
    movie: &MovieNfo,
    options: &MovieNfoOptions,
    root: &mut Element,
    plain_fanart: bool,
) {
    let text = |name: &str, value: &str| Element::text(name, value);
    root.push(match &movie.movie_set {
        Some(set) => text("set", &set.title),
        None => Element::new("set"),
    });
    root.push(text("plot", &movie.plot));
    if let Some(outline) = outline(movie, options) {
        root.push(text("outline", &outline));
    }
    root.push(text("tagline", &movie.tagline));
    root.push(text("runtime", &movie.runtime_minutes.to_string()));
    if options.artwork_urls {
        root.push(text("thumb", artwork_url_text(&movie.artwork.poster)));
        let fanart = artwork_url_text(&movie.artwork.fanart);
        root.push(if plain_fanart {
            text("fanart", fanart)
        } else if fanart.is_empty() {
            Element::new("fanart")
        } else {
            Element::new("fanart").child(text("thumb", fanart))
        });
    }
}

/// `premiered watched playcount [lastplayed]`.
fn watch_state(movie: &MovieNfo, root: &mut Element) {
    let text = |name: &str, value: &str| Element::text(name, value);
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
}

/// `actor* producer*`.
fn people(movie: &MovieNfo, options: &MovieNfoOptions, root: &mut Element) {
    for credit in credits_of(movie, CreditKind::Actor) {
        root.push(actor("actor", credit, options, true));
    }
    for credit in credits_of(movie, CreditKind::Producer) {
        root.push(producer(credit, options));
    }
}

/// The URL of the first trailer marked for the NFO, as stored.
fn raw_trailer(movie: &MovieNfo) -> String {
    movie
        .trailers
        .iter()
        .find(|t| t.in_nfo && !t.url.trim().is_empty())
        .map(|t| t.url.trim().to_owned())
        .unwrap_or_default()
}

/// `[dateadded] [lockdata] [fileinfo] <unmanaged elements> <private tags>`.
fn tail(
    movie: &MovieNfo,
    options: &MovieNfoOptions,
    root: &mut Element,
    file_info: fn(&MovieNfo, &MovieNfoOptions) -> Element,
) {
    if options.write_date_added {
        root.push(Element::text(
            "dateadded",
            date_added(movie, options).unwrap_or_default(),
        ));
    }
    if options.lockdata {
        root.push(Element::text("lockdata", "true"));
    }
    if options.fileinfo {
        root.push(file_info(movie, options));
    }
    for unknown in &movie.unknown_elements {
        root.push(unknown.clone());
    }
    private_tags(movie, options, root);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xbmc_spells_hevc_in_capitals_and_keeps_other_codecs() {
        assert_eq!(xbmc_video_codec("hevc"), "HEVC");
        assert_eq!(xbmc_video_codec(" H265 "), "HEVC");
        assert_eq!(xbmc_video_codec("h264"), "h264");
        assert_eq!(xbmc_video_codec("mpeg-2"), "mpeg-2");
    }

    #[test]
    fn short_floats_keep_one_decimal() {
        assert_eq!(short_float(2.4), "2.4");
        assert_eq!(short_float(1.78), "1.78");
        assert_eq!(short_float(0.0), "0.0");
        assert_eq!(short_float(2.0), "2.0");
    }
}
