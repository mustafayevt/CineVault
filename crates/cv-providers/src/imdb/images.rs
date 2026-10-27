//! IMDb's image index pages to artwork candidates.
//!
//! `title/{id}/mediaindex/?contentTypes=poster` and `...=still_frame` list a title's images in
//! `props.pageProps.contentData.data.title.all_images.edges[].node` (`url`, `width`, `height`).
//! Posters keep their portrait images only (IMDb files landscape pictures under posters too), and
//! still frames keep their landscape images only, as fanart. The image index has no language and
//! no votes; the title page's main poster is the one image IMDb vouches for.

use cv_domain::MediaFileType;
use serde_json::Value;

use super::next_data::{self, ParseError, number, text};
use crate::artwork::presort;
use crate::model::{ArtworkCandidate, RequestOptions};

/// The score the main poster gets so it sorts before the rest of IMDb's images.
pub(super) const MAIN_POSTER_SCORE: f32 = 10.0;

/// One image of an index page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Image {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

impl Image {
    /// Reads `{url, width, height}`. Anything but an `http(s)` URL is not an image.
    pub(super) fn read(value: &Value) -> Option<Self> {
        let url = text(value, "/url")
            .filter(|u| u.starts_with("https://") || u.starts_with("http://"))?;
        let size = |path| {
            number(value, path)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0)
        };
        Some(Self {
            url: url.to_owned(),
            width: size("/width"),
            height: size("/height"),
        })
    }

    /// Whether the picture is taller than wide. An image without a size counts as neither.
    fn portrait(&self) -> bool {
        self.height > self.width
    }

    fn landscape(&self) -> bool {
        self.width > self.height
    }
}

/// The images of an index page. A page whose structure has no image list is a
/// [`ParseError`]; an empty list is a title with no images.
pub(super) fn parse_index(html: &str) -> Result<Vec<Image>, ParseError> {
    let data = next_data::extract(html)?;
    let props = next_data::page_props(&data)?;
    let edges = props
        .pointer("/contentData/data/title/all_images/edges")
        .and_then(Value::as_array)
        .ok_or(ParseError::Missing("image list"))?;
    Ok(edges
        .iter()
        .filter_map(|edge| edge.get("node").and_then(Image::read))
        .collect())
}

/// Poster candidates: the main poster first (when the title page has one), then the portrait
/// images of the index, each image once.
pub(super) fn posters(
    main: Option<&Image>,
    index: &[Image],
    options: &RequestOptions,
) -> Vec<ArtworkCandidate> {
    let mut out = Vec::new();
    if let Some(main) = main {
        out.push(candidate(MediaFileType::Poster, main, MAIN_POSTER_SCORE));
    }
    for image in index.iter().filter(|i| i.portrait()) {
        if !out.iter().any(|c| c.url == image.url) {
            out.push(candidate(MediaFileType::Poster, image, 0.0));
        }
    }
    presort(&mut out, options);
    out
}

/// Fanart candidates: the landscape still frames of the index, each image once.
pub(super) fn fanart(index: &[Image], options: &RequestOptions) -> Vec<ArtworkCandidate> {
    let mut out: Vec<ArtworkCandidate> = Vec::new();
    for image in index.iter().filter(|i| i.landscape()) {
        if !out.iter().any(|c| c.url == image.url) {
            out.push(candidate(MediaFileType::Fanart, image, 0.0));
        }
    }
    presort(&mut out, options);
    out
}

fn candidate(kind: MediaFileType, image: &Image, score: f32) -> ArtworkCandidate {
    ArtworkCandidate {
        kind,
        url: image.url.clone(),
        language: None,
        width: image.width,
        height: image.height,
        score,
        popularity: crate::model::Popularity::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(url: &str, width: u32, height: u32) -> Image {
        Image {
            url: url.to_owned(),
            width,
            height,
        }
    }

    #[test]
    fn the_main_poster_comes_first_and_is_not_repeated() {
        let main = image("https://i.test/main.jpg", 2000, 3000);
        let index = [
            image("https://i.test/a.jpg", 1000, 1500),
            image("https://i.test/main.jpg", 2000, 3000),
            image("https://i.test/wide.jpg", 3000, 2000),
            image("https://i.test/b.jpg", 800, 1200),
        ];
        let list = posters(Some(&main), &index, &RequestOptions::default());
        let urls: Vec<&str> = list
            .iter()
            .map(|c| c.url.rsplit('/').next().unwrap())
            .collect();
        assert_eq!(urls, ["main.jpg", "a.jpg", "b.jpg"]);
        assert_eq!(list[0].score, MAIN_POSTER_SCORE);
        assert!(list[1..].iter().all(|c| c.score == 0.0));
    }

    #[test]
    fn fanart_keeps_landscape_images_only() {
        let index = [
            image("https://i.test/a.jpg", 1920, 1080),
            image("https://i.test/tall.jpg", 1000, 1500),
            image("https://i.test/square.jpg", 1000, 1000),
            image("https://i.test/unsized.jpg", 0, 0),
            image("https://i.test/a.jpg", 1920, 1080),
        ];
        let list = fanart(&index, &RequestOptions::default());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, MediaFileType::Fanart);
    }
}
