//! The three live examples under the template editor.
//!
//! They show the templates on real titles of the library where it has the edge cases (an
//! edition, no HDR, a colon in the title) and on built-in samples where it does not.

use cv_domain::{AudioStream, Edition, Item, ItemDetail, MediaFile, MediaFileType, MediaSource};
use cv_domain::{EpisodeNumber, ItemId};
use cv_rename::{MovieRenamer, TvRenamer};

use super::load::LoadedMovie;
use super::model::{Example, ExampleKind};

/// Renders the examples under `renamer`.
pub(crate) fn examples(movies: &[&LoadedMovie], renamer: &MovieRenamer) -> Vec<Example> {
    let min_set = if renamer.settings().create_movieset_for_single_movie {
        0
    } else {
        1
    };
    [ExampleKind::Edition, ExampleKind::NoHdr, ExampleKind::Colon]
        .into_iter()
        .filter_map(|kind| {
            let library = movies.iter().find(|m| fits(kind, &m.item));
            let (item, set, sample) = match library {
                Some(m) => (
                    m.item.clone(),
                    m.set.clone().filter(|s| s.movies.len() > min_set),
                    false,
                ),
                None => (sample(kind), None, true),
            };
            let folder = renamer.folder_name(&item, set.as_ref()).ok()?;
            let file = renamer.file_name(&item, set.as_ref()).ok()?;
            Some(Example {
                kind,
                title: item.core.title.clone(),
                year: item.core.year,
                folder,
                file,
                sample,
            })
        })
        .collect()
}

/// The TV episode example: a built-in sample show, so the episode template always has one to
/// show, whether or not the library holds a show. `folder` is the show and season folders.
pub(crate) fn episode_example(renamer: &TvRenamer) -> Option<Example> {
    let mut show = Item::show("Severance");
    show.core.year = 2022;
    show.core.ids.set("tmdb", "95396");
    let mut episode = Item::episode(ItemId(0), "Hello, Ms. Cobel");
    if let ItemDetail::Episode(fields) = &mut episode.detail {
        fields.numbers.set(EpisodeNumber::aired(2, 1));
    }
    let mut file = video(1920, 1080, &["HDR10"], "DTS", 6);
    file.container_format = "Matroska".to_owned();
    episode.core.media_files.push(file);
    if let ItemDetail::Episode(fields) = &mut episode.detail {
        fields.media_source = MediaSource::Bluray;
    }
    let name = renamer.episode_file_name(&show, &[&episode]).ok()?;
    let show_folder = renamer.show_folder_name(&show).ok()?;
    let season_folder = renamer.season_folder_name(&show, &episode).ok()?;
    Some(Example {
        kind: ExampleKind::Episode,
        title: show.core.title.clone(),
        year: show.core.year,
        folder: format!("{show_folder}/{season_folder}"),
        file: name,
        sample: true,
    })
}

/// Whether `item` shows the edge case of `kind`.
fn fits(kind: ExampleKind, item: &Item) -> bool {
    let ItemDetail::Movie(fields) = &item.detail else {
        return false;
    };
    match kind {
        ExampleKind::Edition => fields.edition != Edition::None,
        ExampleKind::NoHdr => item.core.media_files.iter().any(|f| {
            f.kind == MediaFileType::Video && f.has_media_info() && f.hdr_formats.is_empty()
        }),
        ExampleKind::Colon => item.core.title.contains(':'),
        ExampleKind::Plain | ExampleKind::Episode => true,
    }
}

fn video(width: u32, height: u32, hdr: &[&str], codec: &str, channels: u32) -> MediaFile {
    let mut file = MediaFile::new(MediaFileType::Video, "sample", "sample.mkv");
    file.container_format = "Matroska".to_owned();
    file.width = width;
    file.height = height;
    file.hdr_formats = hdr.iter().map(|h| (*h).to_owned()).collect();
    file.audio_streams.push(AudioStream {
        codec: codec.to_owned(),
        language: "en".to_owned(),
        channels,
        ..AudioStream::default()
    });
    file
}

/// A built-in title for an edge case the library does not have.
fn sample(kind: ExampleKind) -> Item {
    let (title, year, tmdb, file) = match kind {
        ExampleKind::Edition => (
            "Heat",
            1995,
            "949",
            video(3840, 2160, &["HDR10"], "DTS-HD MA", 8),
        ),
        ExampleKind::NoHdr => ("Arrival", 2016, "329865", video(1920, 1080, &[], "DTS", 6)),
        ExampleKind::Colon | ExampleKind::Plain | ExampleKind::Episode => (
            "Dune: Part Two",
            2024,
            "693134",
            video(3840, 2160, &["Dolby Vision", "HDR10"], "TrueHD Atmos", 8),
        ),
    };
    let mut item = Item::movie(title);
    item.core.year = year;
    item.core.ids.set("tmdb", tmdb);
    item.core.media_files.push(file);
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.media_source = MediaSource::Bluray;
        if kind == ExampleKind::Edition {
            fields.edition = Edition::DirectorsCut;
        }
    }
    item
}
