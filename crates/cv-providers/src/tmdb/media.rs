//! Artwork and trailer candidates from TMDB's `images` and `videos` responses.

use cv_domain::{MediaFileType, Trailer};

use super::api::{Image, Images, Text, Video, Videos};
use super::convert::{image_url, parse_date};
use super::language::Locale;
use crate::artwork::popularity_value;
use crate::model::{ArtworkCandidate, Popularity};

/// Sizes offered per image type, largest first. `original` is always offered.
const POSTER_SIZES: &[(&str, u32)] = &[("w780", 780), ("w500", 500), ("w342", 342), ("w185", 185)];
const BACKDROP_SIZES: &[(&str, u32)] = &[("w1280", 1280), ("w300", 300)];
const LOGO_SIZES: &[(&str, u32)] = &[("w500", 500), ("w300", 300)];
const STILL_SIZES: &[(&str, u32)] = &[("w300", 300)];

/// The width given to a vector logo that is served as PNG, which has no pixel size of its own.
const SVG_ASSUMED_WIDTH: u32 = 2000;

/// Which images to read from a response, as which artwork type.
#[derive(Clone, Copy)]
pub(super) enum Slot {
    Posters(MediaFileType),
    Backdrops(MediaFileType),
    Logos(MediaFileType),
    Stills(MediaFileType),
}

fn rank(language: &Option<String>, preferred: &Locale) -> u8 {
    match language.as_deref().map(str::trim) {
        Some(l) if l.eq_ignore_ascii_case(&preferred.language) => 0,
        Some(l) if l.eq_ignore_ascii_case("en") => 1,
        None | Some("") | Some("xx") => 2,
        Some(_) => 3,
    }
}

/// The votes of an image.
fn votes(image: &Image) -> Popularity {
    Popularity::Rated {
        average: image.vote_average.unwrap_or(0.0),
        votes: image.vote_count.unwrap_or(0),
    }
}

/// The candidates for one slot, best first: the preferred language, then English, then images
/// without text, then the rest; each group by its votes (a Bayesian average, so a few high votes
/// do not beat many good ones), highest first. Every image comes
/// as the sizes TMDB serves, original first.
pub(super) fn candidates(
    base: &str,
    images: &Images,
    slot: Slot,
    preferred: &Locale,
) -> Vec<ArtworkCandidate> {
    let (list, kind, sizes): (&[Image], MediaFileType, &[(&str, u32)]) = match slot {
        Slot::Posters(kind) => (&images.posters, kind, POSTER_SIZES),
        Slot::Backdrops(kind) => (&images.backdrops, kind, BACKDROP_SIZES),
        Slot::Logos(kind) => (&images.logos, kind, LOGO_SIZES),
        Slot::Stills(kind) => (&images.stills, kind, STILL_SIZES),
    };
    let mut ordered: Vec<&Image> = list
        .iter()
        .filter(|i| !i.file_path.text().trim().is_empty())
        .collect();
    ordered.sort_by(|a, b| {
        rank(&a.iso_639_1, preferred)
            .cmp(&rank(&b.iso_639_1, preferred))
            .then_with(|| popularity_value(votes(b)).total_cmp(&popularity_value(votes(a))))
            .then_with(|| b.vote_count.unwrap_or(0).cmp(&a.vote_count.unwrap_or(0)))
    });

    let mut out = Vec::new();
    for image in ordered {
        let path = image.file_path.text().trim();
        let vector = path.to_ascii_lowercase().ends_with(".svg");
        let (path, width, height) = if vector {
            // TMDB serves a PNG rendering of a vector logo.
            let png = format!("{}.png", &path[..path.len() - 4]);
            let width = SVG_ASSUMED_WIDTH;
            let height = if image.width > 0 {
                scale(image.height, width, image.width)
            } else {
                0
            };
            (png, width, height)
        } else {
            (path.to_owned(), image.width, image.height)
        };
        let language = image
            .iso_639_1
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned);
        let score = image.vote_average.unwrap_or(0.0);
        let popularity = Popularity::Rated {
            average: score,
            votes: image.vote_count.unwrap_or(0),
        };
        let mut push = |size: &str, width: u32, height: u32| {
            if let Some(url) = image_url(base, size, &path) {
                out.push(ArtworkCandidate {
                    kind,
                    url,
                    language: language.clone(),
                    width,
                    height,
                    score,
                    popularity,
                });
            }
        };
        push("original", width, height);
        for (name, size_width) in sizes {
            // A size above the original would only be an upscale.
            if width == 0 || *size_width < width {
                let h = if width == 0 {
                    0
                } else {
                    scale(height, *size_width, width)
                };
                push(name, if width == 0 { 0 } else { *size_width }, h);
            }
        }
    }
    out
}

/// `value * to / from`, rounded.
fn scale(value: u32, to: u32, from: u32) -> u32 {
    if from == 0 {
        return 0;
    }
    let scaled = (u64::from(value) * u64::from(to) + u64::from(from) / 2) / u64::from(from);
    u32::try_from(scaled).unwrap_or(u32::MAX)
}

/// Trailers from one or more `videos` responses: type `Trailer` only, without Apple-hosted
/// clips, each video once. Quality is the video's height (`1080p`).
pub(super) fn trailers(responses: &[&Videos]) -> Vec<Trailer> {
    let mut out: Vec<Trailer> = Vec::new();
    for response in responses {
        for video in &response.results {
            if !video.kind.text().eq_ignore_ascii_case("trailer") {
                continue;
            }
            let site = video.site.text().trim();
            if site.eq_ignore_ascii_case("apple") {
                continue;
            }
            let key = video.key.text().trim();
            let Some(url) = video_url(site, key) else {
                continue;
            };
            let id = if video.id.text().trim().is_empty() {
                key.to_owned()
            } else {
                video.id.text().trim().to_owned()
            };
            if out.iter().any(|t| t.id == id || t.url == url) {
                continue;
            }
            out.push(Trailer {
                id,
                name: video.name.text().trim().to_owned(),
                url,
                quality: if video.size > 0 {
                    format!("{}p", video.size)
                } else {
                    String::new()
                },
                language: language_tag(video),
                provider: "tmdb".to_owned(),
                in_nfo: false,
                date: parse_date(video.published_at.text()),
                scraped_by: "tmdb".to_owned(),
                length_secs: 0,
            });
        }
    }
    out
}

fn video_url(site: &str, key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    match site.to_ascii_lowercase().as_str() {
        "youtube" => Some(format!("https://www.youtube.com/watch?v={key}")),
        "vimeo" => Some(format!("https://vimeo.com/{key}")),
        _ => None,
    }
}

/// `iso_639_1-iso_3166_1`, or just the language when TMDB gives no country.
fn language_tag(video: &Video) -> String {
    let language = video.iso_639_1.text().trim();
    let country = video.iso_3166_1.text().trim();
    match (language.is_empty(), country.is_empty()) {
        (true, _) => String::new(),
        (false, true) => language.to_owned(),
        (false, false) => format!("{language}-{country}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(path: &str, language: Option<&str>, width: u32, height: u32, vote: f32) -> Image {
        Image {
            file_path: Some(path.into()),
            width,
            height,
            iso_639_1: language.map(str::to_owned),
            vote_average: Some(vote),
            vote_count: Some(1),
        }
    }

    #[test]
    fn posters_come_in_every_smaller_size_original_first() {
        let images = Images {
            posters: vec![image("/p.jpg", Some("en"), 2000, 3000, 5.0)],
            ..Images::default()
        };
        let preferred = Locale::parse("en").unwrap();
        let list = candidates(
            "https://i.test/t/p/",
            &images,
            Slot::Posters(MediaFileType::Poster),
            &preferred,
        );
        let urls: Vec<&str> = list.iter().map(|c| c.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://i.test/t/p/original/p.jpg",
                "https://i.test/t/p/w780/p.jpg",
                "https://i.test/t/p/w500/p.jpg",
                "https://i.test/t/p/w342/p.jpg",
                "https://i.test/t/p/w185/p.jpg",
            ]
        );
        assert_eq!((list[0].width, list[0].height), (2000, 3000));
        assert_eq!((list[2].width, list[2].height), (500, 750));
    }

    #[test]
    fn sizes_above_the_original_are_not_offered() {
        let images = Images {
            posters: vec![image("/small.jpg", Some("en"), 400, 600, 5.0)],
            ..Images::default()
        };
        let list = candidates(
            "https://i.test/t/p/",
            &images,
            Slot::Posters(MediaFileType::Poster),
            &Locale::parse("en").unwrap(),
        );
        let sizes: Vec<u32> = list.iter().map(|c| c.width).collect();
        assert_eq!(sizes, vec![400, 342, 185]);
    }

    #[test]
    fn language_order_then_votes() {
        let images = Images {
            backdrops: vec![
                image("/de.jpg", Some("de"), 1920, 1080, 9.0),
                image("/none.jpg", None, 1920, 1080, 8.0),
                image("/en-low.jpg", Some("en"), 1920, 1080, 4.0),
                image("/en-high.jpg", Some("en"), 1920, 1080, 6.0),
                image("/tr.jpg", Some("tr"), 1920, 1080, 1.0),
            ],
            ..Images::default()
        };
        let list = candidates(
            "https://i.test/t/p/",
            &images,
            Slot::Backdrops(MediaFileType::Fanart),
            &Locale::parse("tr").unwrap(),
        );
        let originals: Vec<&str> = list
            .iter()
            .filter(|c| c.url.contains("/original/"))
            .map(|c| c.url.rsplit('/').next().unwrap())
            .collect();
        assert_eq!(
            originals,
            vec!["tr.jpg", "en-high.jpg", "en-low.jpg", "none.jpg", "de.jpg"]
        );
    }

    #[test]
    fn votes_are_kept_and_many_good_votes_beat_one_high_vote() {
        let voted = |path: &str, vote: f32, count: u32| Image {
            vote_count: Some(count),
            ..image(path, Some("en"), 2000, 3000, vote)
        };
        let images = Images {
            posters: vec![voted("/one-ten.jpg", 10.0, 1), voted("/fifty.jpg", 5.6, 50)],
            ..Images::default()
        };
        let list = candidates(
            "https://i.test/t/p/",
            &images,
            Slot::Posters(MediaFileType::Poster),
            &Locale::parse("en").unwrap(),
        );
        assert!(list[0].url.ends_with("/fifty.jpg"));
        assert_eq!(
            list[0].popularity,
            Popularity::Rated {
                average: 5.6,
                votes: 50
            }
        );
        assert!(list.iter().all(|c| c.popularity.count() > 0));
    }

    #[test]
    fn a_vector_logo_is_served_as_png_at_an_assumed_width() {
        let images = Images {
            logos: vec![image("/logo.svg", Some("en"), 500, 100, 5.0)],
            ..Images::default()
        };
        let list = candidates(
            "https://i.test/t/p/",
            &images,
            Slot::Logos(MediaFileType::Clearlogo),
            &Locale::parse("en").unwrap(),
        );
        assert_eq!(list[0].url, "https://i.test/t/p/original/logo.png");
        assert_eq!((list[0].width, list[0].height), (2000, 400));
        assert!(list.iter().all(|c| c.url.ends_with(".png")));
    }

    #[test]
    fn trailers_keep_only_trailers_and_each_video_once() {
        let first = Videos {
            results: vec![
                Video {
                    id: Some("v1".into()),
                    key: Some("abc".into()),
                    name: Some("Official Trailer".into()),
                    site: Some("YouTube".into()),
                    size: 1080,
                    kind: Some("Trailer".into()),
                    iso_639_1: Some("de".into()),
                    iso_3166_1: Some("DE".into()),
                    published_at: Some("2024-03-01T10:00:00.000Z".into()),
                },
                Video {
                    id: Some("v2".into()),
                    key: Some("def".into()),
                    site: Some("YouTube".into()),
                    size: 1080,
                    kind: Some("Teaser".into()),
                    ..Video::default()
                },
                Video {
                    id: Some("v3".into()),
                    key: Some("ghi".into()),
                    site: Some("Apple".into()),
                    kind: Some("Trailer".into()),
                    ..Video::default()
                },
            ],
        };
        let second = Videos {
            results: vec![
                Video {
                    id: Some("v1".into()),
                    key: Some("abc".into()),
                    site: Some("YouTube".into()),
                    size: 1080,
                    kind: Some("Trailer".into()),
                    ..Video::default()
                },
                Video {
                    id: Some("v4".into()),
                    key: Some("jkl".into()),
                    name: Some("Trailer".into()),
                    site: Some("YouTube".into()),
                    size: 720,
                    kind: Some("Trailer".into()),
                    iso_639_1: Some("en".into()),
                    iso_3166_1: Some("US".into()),
                    ..Video::default()
                },
            ],
        };
        let list = trailers(&[&first, &second]);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].url, "https://www.youtube.com/watch?v=abc");
        assert_eq!(list[0].quality, "1080p");
        assert_eq!(list[0].language, "de-DE");
        assert_eq!(list[0].provider, "tmdb");
        assert_eq!(list[0].date, cv_domain::CalendarDate::new(2024, 3, 1));
        assert_eq!(list[1].url, "https://www.youtube.com/watch?v=jkl");
        assert_eq!(list[1].language, "en-US");
    }
}
