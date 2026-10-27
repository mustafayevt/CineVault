//! The season NFO writer.
//!
//! Seasons are written in one layout whatever the TV format is:
//!
//! ```text
//! seasonnumber title showtitle sorttitle year plot [thumb*] [fanart] tvdbid imdbid tmdbid
//! uniqueid* premiered [lockdata] <elements CineVault does not manage>
//! <!-- CineVault meta data -->  user_note
//! ```
//!
//! `title` is the season's own name, else `Season N`; `sorttitle` is `Season NN`. The IDs are the
//! ones of the show (a season has none of its own) and `uniqueid` has no `default` attribute.
//! `year` is always empty, as the fixtures pin it.

use crate::kodi_movie::product_name;
use crate::tv::{SeasonNfo, ShowNfo, TvNfoOptions};
use crate::tv_common::unique_ids;
use crate::xml::{Document, Element};

/// Builds the NFO document of a season of `show`.
pub fn season_document(show: &ShowNfo, season: &SeasonNfo, options: &TvNfoOptions) -> Document {
    let common = &options.common;
    let text = |name: &str, value: &str| Element::text(name, value);

    let title = if season.title.trim().is_empty() {
        format!("Season {}", season.number)
    } else {
        season.title.clone()
    };
    let mut root = Element::new("season");
    root.push(text("seasonnumber", &season.number.to_string()));
    root.push(text("title", &title));
    root.push(text("showtitle", &show.title));
    root.push(text("sorttitle", &format!("Season {:02}", season.number)));
    root.push(text("year", ""));
    root.push(text("plot", &season.plot));
    if common.artwork_urls {
        let art = &season.artwork;
        for (aspect, url) in [
            ("poster", &art.poster),
            ("banner", &art.banner),
            ("landscape", &art.thumb),
        ] {
            if !url.trim().is_empty() {
                root.push(Element::text("thumb", url.trim()).attr("aspect", aspect));
            }
        }
        if !art.fanart.trim().is_empty() {
            root.push(Element::new("fanart").child(text("thumb", art.fanart.trim())));
        }
    }
    root.push(text("tvdbid", show.ids.get("tvdb").unwrap_or_default()));
    root.push(text("imdbid", show.ids.get("imdb").unwrap_or_default()));
    root.push(text("tmdbid", show.ids.get("tmdb").unwrap_or_default()));
    for id in unique_ids(&show.ids, None) {
        root.push(id);
    }
    root.push(text(
        "premiered",
        &season
            .first_aired
            .map(|d| d.to_string())
            .unwrap_or_default(),
    ));
    if common.lockdata {
        root.push(text("lockdata", "true"));
    }
    for unknown in &season.unknown_elements {
        root.push(unknown.clone());
    }
    root.push_comment(format!("{} meta data", product_name(&common.generator)));
    root.push(text("user_note", &season.note));

    let mut document = Document::new(root);
    document.comment = Some(if common.created_on.is_empty() {
        format!("created by {}", common.generator)
    } else {
        format!("created on {} by {}", common.created_on, common.generator)
    });
    document
}

/// Renders the NFO of a season: UTF-8 text, two-space indentation, CRLF line ends.
pub fn render_season_nfo(show: &ShowNfo, season: &SeasonNfo, options: &TvNfoOptions) -> String {
    season_document(show, season, options).render()
}
