//! The episode NFO writer (`<episodedetails>`), including files that hold several episodes.
//!
//! A file with several episodes holds one `<episodedetails>` block per episode, one after the
//! other, under a single XML declaration and a single header comment. Element order, as in
//! `tests/compat/expected/nfo/tv-*`:
//!
//! ```text
//! title originaltitle showtitle season episode [displayseason displayepisode] id uniqueid*
//! ratings userrating plot [outline] runtime [thumb] mpaa premiered aired watched playcount
//! [lastplayed] studio* tag* credits* director* actor* [trailer] [dateadded] [lockdata]
//! [fileinfo] epbookmark code [videoassettitle hasvideoversions] [episodenumberend]
//! <elements CineVault does not manage>  <!-- CineVault meta data -->
//! source edition original_filename user_note [episode_groups] [english_title] [crc32] crew*
//! ```
//!
//! The XBMC layout has a flat `<rating>` and `<votes>`, the older `<fileinfo>` and no edition
//! elements. The Emby layout lists `credits` and `director` after the actors and adds
//! `episodenumberend` (the highest episode of the file) to every block of a multi-episode file.
//!
//! Details the fixtures pin down:
//!
//! * the runtime, and the duration in `<fileinfo>`, are those of the video file divided by the
//!   number of episodes in it; the runtime of the episode is used only when the file has none;
//! * `<trailer>` is written (empty, or with the old NFO's text) only with the option "write all
//!   actors", never with the option "write trailer", as the
//!   fixtures pin it;
//! * `<episode_groups>` lists only the orderings that have a name and is left out when there is
//!   none.

use cv_domain::{CreditKind, Edition};

use crate::kodi_movie::{
    edition_title, fileinfo_of, header_comment, named_person, non_blank, product_name,
};
use crate::legacy_movie::xbmc_fileinfo_of;
use crate::movie::{DateAddedField, MovieNfoOptions};
use crate::streams::VideoInfo;
use crate::tv::{EpisodeNfo, TvConnector, TvFlavor, TvNfoOptions};
use crate::tv_common::{
    crew_of, flat_rating, of_kind, ordering_id, outline, preferred_rating, ratings_element,
    tv_actor, unique_ids, user_rating,
};
use crate::xml::{Document, Element};

/// Builds the NFO document of one video file that holds `episodes` (at least one).
pub fn episode_document(
    connector: TvConnector,
    episodes: &[EpisodeNfo],
    options: &TvNfoOptions,
) -> Document {
    let flavor = connector.episode_flavor();
    let common = &options.common;
    let roots: Vec<Element> = episodes
        .iter()
        .map(|episode| episode_element(flavor, episode, episodes, options))
        .collect();
    Document {
        comment: Some(header_comment(common, connector.label())),
        roots,
    }
}

/// Renders the NFO of one video file: UTF-8 text, two-space indentation, CRLF line ends.
pub fn render_episode_nfo(
    connector: TvConnector,
    episodes: &[EpisodeNfo],
    options: &TvNfoOptions,
) -> String {
    episode_document(connector, episodes, options).render()
}

fn episode_element(
    flavor: TvFlavor,
    episode: &EpisodeNfo,
    file_episodes: &[EpisodeNfo],
    options: &TvNfoOptions,
) -> Element {
    let common = &options.common;
    let text = |name: &str, value: &str| Element::text(name, value);
    let xbmc = flavor == TvFlavor::Xbmc;
    let emby = flavor == TvFlavor::Emby;
    let in_file = file_episodes.len().max(1) as u32;

    let mut root = Element::new("episodedetails");
    root.push(text("title", &episode.title));
    root.push(text("originaltitle", &episode.original_title));
    root.push(text("showtitle", &episode.show_title));
    root.push(text("season", &episode.season.to_string()));
    root.push(text("episode", &episode.episode.to_string()));
    if let Some((season, number)) = episode.display {
        root.push(text("displayseason", &season.to_string()));
        root.push(text("displayepisode", &number.to_string()));
    }
    root.push(text("id", episode.ids.get("tvdb").unwrap_or_default()));
    for id in unique_ids(&episode.ids, Some(&["imdb", "tvdb", "tmdb"])) {
        root.push(id);
    }
    if xbmc {
        let (rating, votes) = flat_rating(&episode.ratings, common);
        root.push(text("rating", &rating));
        root.push(text("userrating", &user_rating(&episode.ratings)));
        root.push(text("votes", &votes));
    } else {
        let main = preferred_rating(&episode.ratings, common);
        root.push(ratings_element(main, &episode.ratings));
        root.push(text("userrating", &user_rating(&episode.ratings)));
    }
    root.push(text("plot", &episode.plot));
    if let Some(outline) = outline(
        &episode.outline,
        &episode.plot,
        &episode.carried.outline,
        common,
    ) {
        root.push(text("outline", &outline));
    }
    root.push(text(
        "runtime",
        &runtime_minutes(episode, in_file).to_string(),
    ));
    if common.artwork_urls && !episode.thumb.trim().is_empty() {
        root.push(text("thumb", episode.thumb.trim()));
    }
    let certification = crate::certification::format_certification(
        &episode.certification.0,
        common.certification_style,
    );
    root.push(text("mpaa", &certification));
    let aired = episode
        .first_aired
        .map(|d| d.to_string())
        .unwrap_or_default();
    root.push(text("premiered", &aired));
    root.push(text("aired", &aired));
    root.push(text(
        "watched",
        if episode.watched { "true" } else { "false" },
    ));
    root.push(text("playcount", &play_count(episode).to_string()));
    if episode.watched
        && let Some(last) = last_played(episode)
    {
        root.push(text("lastplayed", &last));
    }
    let mut studios: Vec<&str> = non_blank(&episode.studios).collect();
    if common.single_studio {
        studios.truncate(1);
    }
    for studio in studios {
        root.push(text("studio", studio));
    }
    for tag in non_blank(&episode.tags) {
        root.push(text("tag", tag));
    }
    if !emby {
        writers_and_directors(episode, &mut root);
    }
    for credit in episode
        .credits
        .iter()
        .filter(|c| matches!(c.kind, CreditKind::Actor | CreditKind::Guest))
    {
        root.push(tv_actor(credit, common));
    }
    if emby {
        writers_and_directors(episode, &mut root);
    }
    if options.write_all_actors {
        root.push(text("trailer", &episode.carried.trailer));
    }
    if common.write_date_added {
        root.push(text(
            "dateadded",
            &date_added(episode, common).unwrap_or_default(),
        ));
    }
    if common.lockdata {
        root.push(text("lockdata", "true"));
    }
    if common.fileinfo {
        root.push(fileinfo(episode, in_file, xbmc, common.cv_extensions));
    }

    root.push(text("epbookmark", &episode.carried.epbookmark));
    root.push(text("code", &episode.carried.code));
    if !xbmc && episode.edition != Edition::None {
        root.push(text("videoassettitle", &edition_title(&episode.edition)));
        root.push(text("hasvideoversions", "true"));
    }
    if emby
        && file_episodes.len() > 1
        && let Some(last) = file_episodes.iter().max_by_key(|e| (e.season, e.episode))
    {
        root.push(text("episodenumberend", &last.episode.to_string()));
    }
    for unknown in &episode.unknown_elements {
        root.push(unknown.clone());
    }

    private_tags(episode, common, &mut root);
    root
}

fn writers_and_directors(episode: &EpisodeNfo, root: &mut Element) {
    for credit in of_kind(&episode.credits, CreditKind::Writer) {
        root.push(named_person("credits", credit));
    }
    for credit in of_kind(&episode.credits, CreditKind::Director) {
        root.push(named_person("director", credit));
    }
}

/// The duration of the video file that belongs to this episode, in seconds.
fn file_duration(video: &VideoInfo, in_file: u32) -> u32 {
    if in_file > 1 {
        (video.duration_secs as f32 / in_file as f32) as u32
    } else {
        video.duration_secs
    }
}

/// The runtime in minutes: from the video file, else the episode's own.
fn runtime_minutes(episode: &EpisodeNfo, in_file: u32) -> u32 {
    match file_duration(&episode.video, in_file) / 60 {
        0 => episode.runtime_minutes,
        minutes => minutes,
    }
}

fn play_count(episode: &EpisodeNfo) -> u32 {
    if episode.watched {
        episode.play_count.max(episode.carried.play_count).max(1)
    } else {
        0
    }
}

/// The later of the stored and the carried last-played time.
fn last_played(episode: &EpisodeNfo) -> Option<String> {
    [&episode.last_played, &episode.carried.last_played]
        .into_iter()
        .flatten()
        .filter(|t| !t.is_empty())
        .max()
        .cloned()
}

fn date_added(episode: &EpisodeNfo, options: &MovieNfoOptions) -> Option<String> {
    let fallback = || episode.date_added.clone();
    match options.date_added_field {
        DateAddedField::DateAdded => fallback(),
        DateAddedField::FileCreationDate => episode.video.created.clone().or_else(fallback),
        DateAddedField::FileLastModifiedDate => episode.video.modified.clone().or_else(fallback),
        DateAddedField::ReleaseDate => episode
            .first_aired
            .map(|d| format!("{d} 00:00:00"))
            .or_else(fallback),
    }
}

fn fileinfo(episode: &EpisodeNfo, in_file: u32, xbmc: bool, extended: bool) -> Element {
    let duration = file_duration(&episode.video, in_file);
    if xbmc {
        xbmc_fileinfo_of(&episode.video, duration, &[], &episode.external_subtitles)
    } else {
        fileinfo_of(
            &episode.video,
            duration,
            &[],
            &episode.external_subtitles,
            extended,
        )
    }
}

/// CineVault's own tags that close an episode block.
fn private_tags(episode: &EpisodeNfo, options: &MovieNfoOptions, root: &mut Element) {
    let text = |name: &str, value: &str| Element::text(name, value);
    root.push_comment(format!("{} meta data", product_name(&options.generator)));
    root.push(text("source", episode.media_source.name()));
    root.push(text("edition", episode.edition.name()));
    root.push(text("original_filename", &episode.original_filename));
    root.push(text("user_note", &episode.note));
    if let Some(groups) = episode_groups(episode) {
        root.push(groups);
    }
    if !episode.english_title.trim().is_empty() {
        root.push(text("english_title", &episode.english_title));
    }
    if let Some(crc) = episode.crc32.as_deref().filter(|c| !c.is_empty()) {
        root.push(text("crc32", crc));
    }
    for credit in crew_of(&episode.credits) {
        root.push(crate::kodi_movie::crew(credit, options));
    }
}

/// `<episode_groups>`: the numbers in the orderings that have a name. Left out when there is none.
fn episode_groups(episode: &EpisodeNfo) -> Option<Element> {
    let named: Vec<_> = episode
        .numbers
        .iter()
        .filter(|n| !n.group.name.trim().is_empty())
        .collect();
    if named.is_empty() {
        return None;
    }
    let mut element = Element::new("episode_groups");
    for number in named {
        element.push(
            Element::new("group")
                .attr("episode", number.episode.to_string())
                .attr("id", ordering_id(number.group.ordering))
                .attr("name", &number.group.name)
                .attr("season", number.season.to_string()),
        );
    }
    Some(element)
}
