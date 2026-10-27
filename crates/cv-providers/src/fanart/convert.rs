//! Fanart.tv answers to artwork candidates.
//!
//! An answer is one JSON object per title with a list of images per Fanart.tv type name
//! (`movieposter`, `hdmovielogo`, `showbackground`, ...). Every image has an `id`, a `url`, a
//! `lang` (`00` or empty for "no text") and `likes` (a number written as text); season images also
//! have a `season` (a number or `all`). The answer carries no image sizes, so each type gets the
//! size Fanart.tv asks uploaders to deliver.

use cv_domain::MediaFileType;
use serde_json::Value;

use crate::artwork::presort;
use crate::model::{ArtworkCandidate, MediaKind, Popularity, RequestOptions};

/// A Fanart.tv type name and what it becomes.
struct Field {
    name: &'static str,
    kind: MediaFileType,
    /// The size Fanart.tv asks for this type, in pixels.
    size: (u32, u32),
    /// A lower-definition variant of a type that also has an HD one.
    standard_definition: bool,
}

const fn field(
    name: &'static str,
    kind: MediaFileType,
    width: u32,
    height: u32,
    standard_definition: bool,
) -> Field {
    Field {
        name,
        kind,
        size: (width, height),
        standard_definition,
    }
}

const MOVIE_FIELDS: &[Field] = &[
    field("movieposter", MediaFileType::Poster, 1000, 1426, false),
    field("moviebackground", MediaFileType::Fanart, 1920, 1080, false),
    field("hdmovielogo", MediaFileType::Clearlogo, 800, 310, false),
    field("movielogo", MediaFileType::Clearlogo, 400, 155, true),
    field("hdmovieclearart", MediaFileType::Clearart, 1000, 562, false),
    field("movieart", MediaFileType::Clearart, 500, 281, true),
    field("moviedisc", MediaFileType::Disc, 1000, 1000, false),
    field("moviebanner", MediaFileType::Banner, 1000, 185, false),
    field("moviethumb", MediaFileType::Thumb, 1000, 562, false),
];

const SHOW_FIELDS: &[Field] = &[
    field("tvposter", MediaFileType::Poster, 1000, 1426, false),
    field("showbackground", MediaFileType::Fanart, 1920, 1080, false),
    field("hdtvlogo", MediaFileType::Clearlogo, 800, 310, false),
    field("clearlogo", MediaFileType::Clearlogo, 400, 155, true),
    field("hdclearart", MediaFileType::Clearart, 1000, 562, false),
    field("clearart", MediaFileType::Clearart, 500, 281, true),
    field("tvbanner", MediaFileType::Banner, 1000, 185, false),
    field("tvthumb", MediaFileType::Thumb, 500, 281, false),
    field("characterart", MediaFileType::Characterart, 512, 512, false),
];

/// The season lists. A season entry carries its number.
const SEASON_FIELDS: &[Field] = &[
    field(
        "seasonposter",
        MediaFileType::SeasonPoster,
        1000,
        1426,
        false,
    ),
    field("seasonthumb", MediaFileType::SeasonThumb, 500, 281, false),
    field(
        "seasonbanner",
        MediaFileType::SeasonBanner,
        1000,
        185,
        false,
    ),
];

/// Which season entries to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SeasonFilter {
    /// None: the title's own images.
    Title,
    /// One season; `0` is the specials.
    Number(u32),
    /// The images that belong to every season.
    All,
}

impl SeasonFilter {
    /// `season` is the season an image belongs to: `None` for the title's own images.
    fn keeps(self, season: &Option<String>) -> bool {
        match (self, season) {
            (Self::Title, None) => true,
            (Self::Number(n), Some(text)) => text.parse::<u32>() == Ok(n),
            (Self::All, Some(text)) => text.eq_ignore_ascii_case("all"),
            _ => false,
        }
    }
}

/// The candidates of an answer, best first within each type.
///
/// * Posters without text (`lang` `00`) are key art, not posters.
/// * Backgrounds that name a season belong to that season; the others are the title's fanart.
/// * Standard-definition logos and clearart follow the HD ones of their type.
/// * Entries without an `http(s)` URL are dropped.
pub(super) fn candidates(
    answer: &Value,
    kind: MediaKind,
    filter: SeasonFilter,
    options: &RequestOptions,
) -> Vec<ArtworkCandidate> {
    let mut hd = Vec::new();
    let mut standard = Vec::new();
    let title_fields = match kind {
        MediaKind::Movie => MOVIE_FIELDS,
        MediaKind::Show => SHOW_FIELDS,
    };
    let season_fields = match kind {
        MediaKind::Movie => &[][..],
        MediaKind::Show => SEASON_FIELDS,
    };
    for field in title_fields.iter().chain(season_fields) {
        for entry in answer
            .get(field.name)
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
        {
            let Some(candidate) = candidate(field, entry, filter) else {
                continue;
            };
            if field.standard_definition {
                standard.push(candidate);
            } else {
                hd.push(candidate);
            }
        }
    }
    presort(&mut hd, options);
    presort(&mut standard, options);
    // HD first within each type, the types in their usual order.
    let mut out = Vec::with_capacity(hd.len() + standard.len());
    let mut kinds: Vec<MediaFileType> = hd.iter().chain(&standard).map(|c| c.kind).collect();
    kinds.sort();
    kinds.dedup();
    for kind in kinds {
        out.extend(hd.iter().filter(|c| c.kind == kind).cloned());
        out.extend(standard.iter().filter(|c| c.kind == kind).cloned());
    }
    out
}

fn candidate(field: &Field, entry: &Value, filter: SeasonFilter) -> Option<ArtworkCandidate> {
    let url =
        text(entry, "url").filter(|u| u.starts_with("https://") || u.starts_with("http://"))?;
    let mut kind = field.kind;
    // A background that names a season is that season's fanart; every other list that names a
    // season is a season list. A season list entry without a number belongs to no season we can
    // name, so it is dropped.
    let named_season = text(entry, "season");
    let season_list = SEASON_FIELDS.iter().any(|f| f.name == field.name);
    let season = if season_list {
        Some(named_season?)
    } else if field.name == "showbackground" && named_season.is_some() {
        kind = MediaFileType::SeasonFanart;
        named_season
    } else {
        None
    };
    if !filter.keeps(&season) {
        return None;
    }
    let language = text(entry, "lang").filter(|l| l != "00");
    if kind == MediaFileType::Poster && language.is_none() {
        kind = MediaFileType::Keyart;
    }
    let likes = text(entry, "likes")
        .and_then(|l| l.parse::<f32>().ok())
        .filter(|l| l.is_finite())
        .unwrap_or(0.0)
        .max(0.0);
    Some(ArtworkCandidate {
        kind,
        url,
        language,
        width: field.size.0,
        height: field.size.1,
        score: likes,
        // Whole likes; a fraction or a huge number is not something the site sends.
        popularity: Popularity::Liked {
            likes: likes.min(u32::MAX as f32) as u32,
        },
    })
}

/// A non-blank value at `key` as text; Fanart.tv writes numbers as strings, and a real number is
/// accepted too.
fn text(entry: &Value, key: &str) -> Option<String> {
    match entry.get(key)? {
        Value::String(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}
