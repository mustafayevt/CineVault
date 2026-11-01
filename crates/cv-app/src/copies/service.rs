//! The copies service: what the Library, the Movie page and the Compare copies page ask for.

use std::sync::Arc;

use cv_db::repo::CopyMember;
use cv_db::repo::copies::{
    CopyGroup, KeptSetId, copy_groups, copy_sizes, keep_set, remove_kept_set,
};
use cv_db::{Database, DbError};
use cv_domain::{Item, ItemDetail, ItemId, MediaSource, SourceKind};
use cv_media::resolution::resolution_label;
use cv_template::tokens::{audio_channels_dot, media_source_label};

use super::rank::{Quality, best, quality_of};
use crate::hero_audio::hero_audio;
use crate::nfo::model::main_video;
use crate::step_outcomes::now_ms;

/// The number of movies in the groups the user has not kept: the Copies chip's count.
#[must_use]
pub fn open_count(groups: &[CopyGroup]) -> u32 {
    let movies: usize = groups
        .iter()
        .filter(|g| g.is_open())
        .map(|g| g.members.len())
        .sum();
    u32::try_from(movies).unwrap_or(u32::MAX)
}

/// What the media info of one copy says, for the compare rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyMedia {
    /// `2160p`, `1080p`, ...; empty when the frame is below every bucket.
    pub resolution: String,
    /// The HDR formats, such as `Dolby Vision`, `HDR10`; empty for SDR.
    pub hdr_formats: Vec<String>,
    /// The lead audio track's codec and layout, such as `TrueHD Atmos 7.1`; empty without audio.
    pub audio: String,
    pub video_bitrate_kbps: u32,
    /// The running time of the video in whole minutes; `0` when the file does not say.
    pub duration_minutes: u32,
}

/// One card of the Compare copies page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyRow {
    pub id: ItemId,
    pub title: String,
    /// `0` when unknown.
    pub year: u16,
    /// `None` while the copy's media info has not been read ("Not read yet").
    pub media: Option<CopyMedia>,
    /// See [`source_label`]; empty when unknown.
    pub source: String,
    /// Bytes, by the library's size rule.
    pub size_bytes: u64,
    /// The copy's folder.
    pub folder: String,
    /// The name of the copy's main video file.
    pub file_name: String,
    /// The label of the library folder the copy lives on ("Media (D:)"); empty when it was
    /// removed.
    pub source_label: String,
    /// The suggested copy ("Best quality").
    pub best: bool,
}

/// The Compare copies page of one movie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    /// The movie's own copy first, then the others in id order.
    pub copies: Vec<CopyRow>,
    /// The "keep both" decision covering the group, when the user made one.
    pub kept: Option<KeptSetId>,
    /// Every copy shows the same Resolution, HDR, Audio and Source.
    pub look_same: bool,
}

/// The source of a copy as the compare rows and the copy labels name it: `BluRay`, `WEB-DL`,
/// `WEBRip`, `HDTV`, `DVD`, else the library's own name; empty when unknown.
#[must_use]
pub fn source_label(source: &MediaSource) -> String {
    match source {
        MediaSource::UhdBluray | MediaSource::Bluray => "BluRay".to_owned(),
        MediaSource::WebDl => "WEB-DL".to_owned(),
        MediaSource::WebRip => "WEBRip".to_owned(),
        MediaSource::Tv => "HDTV".to_owned(),
        other => media_source_label(other),
    }
}

/// The lead audio track of a video as `<codec> <layout>`; empty without audio.
#[must_use]
pub fn audio_label(video: &cv_domain::MediaFile, original_language: &str) -> String {
    hero_audio(&video.audio_streams, original_language).map_or_else(String::new, |audio| {
        let layout = audio_channels_dot(audio.channels);
        if layout.is_empty() {
            audio.codec.clone()
        } else {
            format!("{} {layout}", audio.codec)
        }
    })
}

/// Reads the copy groups and the compare rows, and records "Keep both" and "Ask again".
pub struct CopiesService {
    db: Arc<Database>,
}

impl CopiesService {
    #[must_use]
    pub fn new(db: Arc<Database>) -> Arc<Self> {
        Arc::new(Self { db })
    }

    /// Every group of copies, with its kept state. Blocking.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn groups(&self) -> Result<Vec<CopyGroup>, DbError> {
        self.db.read(copy_groups)
    }

    /// The number of movies in open groups. Blocking.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn counts(&self) -> Result<u32, DbError> {
        Ok(open_count(&self.groups()?))
    }

    /// The movies of the Copies filter: every open group, its copies ranked best first (unread
    /// copies last, ties by id). Blocking.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn listing(&self) -> Result<Vec<CopyMember>, DbError> {
        let open: Vec<CopyGroup> = self
            .groups()?
            .into_iter()
            .filter(CopyGroup::is_open)
            .collect();
        self.db.read(move |conn| {
            let mut members = Vec::new();
            for group in open {
                let mut ranked = Vec::with_capacity(group.members.len());
                for id in &group.members {
                    let item = cv_db::repo::get_item(conn, *id)?;
                    ranked.push((item.as_ref().and_then(quality), *id));
                }
                ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                members.extend(
                    ranked
                        .into_iter()
                        .enumerate()
                        .map(|(rank, (_, id))| CopyMember {
                            id,
                            group: group.id(),
                            rank: u32::try_from(rank).unwrap_or(u32::MAX),
                        }),
                );
            }
            Ok(members)
        })
    }

    /// The group `id` belongs to, kept or not; `None` when it has no copy. Blocking.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn group_of(&self, id: ItemId) -> Result<Option<CopyGroup>, DbError> {
        Ok(self.groups()?.into_iter().find(|g| g.members.contains(&id)))
    }

    /// The compare rows of `id`'s group; `None` when the movie has no copy left. Blocking.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn compare(&self, id: ItemId) -> Result<Option<Comparison>, DbError> {
        let Some(group) = self.group_of(id)? else {
            return Ok(None);
        };
        let mut order = group.members.clone();
        order.sort_by_key(|member| (*member != id, *member));
        let (items, sizes, labels) = self.db.read(move |conn| {
            let mut items = Vec::with_capacity(order.len());
            let mut labels = Vec::with_capacity(order.len());
            for member in &order {
                if let Some(item) = cv_db::repo::get_item(conn, *member)? {
                    labels.push(
                        cv_db::repo::data_sources::label_of_root(
                            conn,
                            SourceKind::Movie,
                            &item.core.data_source,
                        )?
                        .unwrap_or_default(),
                    );
                    items.push(item);
                }
            }
            Ok((items, copy_sizes(conn, &order)?, labels))
        })?;

        let qualities: Vec<Option<Quality>> = items.iter().map(quality).collect();
        let marked = best(&qualities);
        let copies: Vec<CopyRow> = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let id = item.id.unwrap_or(ItemId(0));
                CopyRow {
                    id,
                    title: item.core.title.clone(),
                    year: item.core.year,
                    media: qualities[index].and_then(|_| media_of(item)),
                    source: movie_source(item),
                    size_bytes: sizes.get(&id).copied().unwrap_or(0),
                    folder: item.core.path.clone(),
                    file_name: main_video(item)
                        .map(|video| video.filename.clone())
                        .unwrap_or_default(),
                    source_label: labels[index].clone(),
                    best: marked == Some(index),
                }
            })
            .collect();
        let look_same = copies.iter().all(|c| c.media.is_some())
            && copies.windows(2).all(|pair| {
                let (a, b) = (&pair[0], &pair[1]);
                let (ma, mb) = (a.media.as_ref(), b.media.as_ref());
                ma.map(|m| (&m.resolution, &m.hdr_formats, &m.audio))
                    == mb.map(|m| (&m.resolution, &m.hdr_formats, &m.audio))
                    && a.source == b.source
            });
        Ok(Some(Comparison {
            copies,
            kept: group.kept,
            look_same,
        }))
    }

    /// "Keep both" / "Keep all": records that the user keeps every one of `ids` as copies.
    /// Returns the decision, which [`Self::undo_keep`] takes back. Blocking.
    ///
    /// # Errors
    /// Fewer than two movies, a movie that does not exist, or the library cannot be written.
    pub fn keep_all(&self, ids: &[ItemId]) -> Result<KeptSetId, DbError> {
        let ids = ids.to_vec();
        let now = now_ms();
        let set = self.db.write(move |tx| keep_set(tx, &ids, now))?;
        tracing::info!(set = set.0, "copies kept");
        Ok(set)
    }

    /// Undo of [`Self::keep_all`]. Blocking.
    ///
    /// # Errors
    /// The library cannot be written.
    pub fn undo_keep(&self, set: KeptSetId) -> Result<(), DbError> {
        self.db.write(move |tx| remove_kept_set(tx, set))
    }

    /// "Ask again": removes every decision that keeps `id`'s group, so it counts again.
    /// Blocking.
    ///
    /// # Errors
    /// The library cannot be read or written.
    pub fn ask_again(&self, id: ItemId) -> Result<(), DbError> {
        self.db.write(move |tx| {
            // An older decision may cover the same members; take them all back.
            while let Some(set) = copy_groups(tx)?
                .into_iter()
                .find(|g| g.members.contains(&id))
                .and_then(|g| g.kept)
            {
                remove_kept_set(tx, set)?;
            }
            Ok(())
        })
    }
}

fn original_language(item: &Item) -> &str {
    &item.core.original_language
}

fn quality(item: &Item) -> Option<Quality> {
    quality_of(main_video(item)?, original_language(item))
}

fn media_of(item: &Item) -> Option<CopyMedia> {
    let video = main_video(item)?;
    Some(CopyMedia {
        resolution: resolution_label(video.width, video.height)
            .unwrap_or_default()
            .to_owned(),
        hdr_formats: video.hdr_formats.clone(),
        audio: audio_label(video, original_language(item)),
        video_bitrate_kbps: video.video_bitrate_kbps,
        duration_minutes: video.duration_secs.div_ceil(60),
    })
}

fn movie_source(item: &Item) -> String {
    match &item.detail {
        ItemDetail::Movie(movie) => source_label(&movie.media_source),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use cv_db::repo::insert_item;
    use cv_domain::{AudioStream, MediaFile, MediaFileType};

    use super::*;

    struct Lab {
        _dir: tempfile::TempDir,
        db: Arc<Database>,
        service: Arc<CopiesService>,
    }

    impl Lab {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
            let service = CopiesService::new(Arc::clone(&db));
            Self {
                _dir: dir,
                db,
                service,
            }
        }

        /// A copy of Example Movie (2020); `width` 0 leaves its media info unread.
        fn copy(
            &self,
            folder: &str,
            width: u32,
            source: MediaSource,
            audio: (&str, u32),
        ) -> ItemId {
            let mut item = Item::movie("Example Movie");
            item.core.year = 2020;
            item.core.path = folder.into();
            item.core.ids.set("tmdb", "1");
            if let ItemDetail::Movie(movie) = &mut item.detail {
                movie.media_source = source;
            }
            let mut video = MediaFile::new(MediaFileType::Video, folder, "Example Movie.mkv");
            video.size_bytes = u64::from(width) * 1000;
            if width > 0 {
                video.container_format = "Matroska".into();
                video.width = width;
                video.height = width * 9 / 16;
                video.video_bitrate_kbps = width * 5;
                video.audio_streams.push(AudioStream {
                    codec: audio.0.into(),
                    channels: audio.1,
                    ..AudioStream::default()
                });
            }
            item.core.media_files.push(video);
            self.db.write(move |tx| insert_item(tx, &item)).unwrap()
        }
    }

    #[test]
    fn the_compare_rows_put_the_opened_copy_first_and_mark_the_best() {
        let lab = Lab::new();
        let web = lab.copy("D:/Movies/dl-1", 1920, MediaSource::WebDl, ("AAC", 6));
        let disc = lab.copy(
            "E:/Films/dl-2",
            3840,
            MediaSource::Bluray,
            ("TrueHD Atmos", 8),
        );

        let page = lab.service.compare(disc).unwrap().unwrap();

        assert_eq!(
            page.copies.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![disc, web]
        );
        let (first, second) = (&page.copies[0], &page.copies[1]);
        assert!(first.best && !second.best);
        let media = first.media.as_ref().unwrap();
        assert_eq!(media.resolution, "2160p");
        assert_eq!(media.audio, "TrueHD Atmos 7.1");
        assert_eq!(media.video_bitrate_kbps, 3840 * 5);
        assert_eq!(first.source, "BluRay");
        assert_eq!(second.source, "WEB-DL");
        assert_eq!(second.media.as_ref().unwrap().resolution, "1080p");
        assert_eq!(first.size_bytes, 3_840_000);
        assert_eq!(second.folder, "D:/Movies/dl-1");
        assert!(!page.look_same);
        assert_eq!(page.kept, None);
    }

    #[test]
    fn an_unread_copy_shows_not_read_yet_and_nothing_is_marked() {
        let lab = Lab::new();
        let read = lab.copy("D:/Movies/a", 3840, MediaSource::Bluray, ("AAC", 6));
        let unread = lab.copy("D:/Movies/b", 0, MediaSource::Unknown, ("", 0));

        let page = lab.service.compare(read).unwrap().unwrap();

        assert!(page.copies.iter().all(|c| !c.best));
        assert_eq!(page.copies[1].id, unread);
        assert_eq!(page.copies[1].media, None);
        assert_eq!(page.copies[1].source, "");
    }

    #[test]
    fn identical_copies_look_the_same() {
        let lab = Lab::new();
        let a = lab.copy("D:/Movies/a", 1920, MediaSource::WebDl, ("AAC", 6));
        lab.copy("D:/Movies/b", 1920, MediaSource::WebDl, ("AAC", 6));

        let page = lab.service.compare(a).unwrap().unwrap();

        assert!(page.look_same);
        assert!(page.copies.iter().all(|c| !c.best), "a tie marks nothing");
    }

    #[test]
    fn keep_all_closes_the_group_and_ask_again_opens_it() {
        let lab = Lab::new();
        let a = lab.copy("D:/Movies/a", 1920, MediaSource::WebDl, ("AAC", 6));
        let b = lab.copy("D:/Movies/b", 3840, MediaSource::Bluray, ("AAC", 6));
        assert_eq!(lab.service.counts().unwrap(), 2);

        let set = lab.service.keep_all(&[a, b]).unwrap();
        assert_eq!(lab.service.counts().unwrap(), 0);
        // A kept group still has its copies: the Movie page line and the page show it.
        assert_eq!(lab.service.group_of(a).unwrap().unwrap().kept, Some(set));
        assert_eq!(lab.service.compare(b).unwrap().unwrap().kept, Some(set));

        lab.service.undo_keep(set).unwrap();
        assert_eq!(lab.service.counts().unwrap(), 2);

        lab.service.keep_all(&[a, b]).unwrap();
        lab.service.keep_all(&[b, a]).unwrap();
        lab.service.ask_again(a).unwrap();
        assert_eq!(lab.service.counts().unwrap(), 2);
    }

    #[test]
    fn the_listing_ranks_each_open_group_best_first() {
        let lab = Lab::new();
        let unread = lab.copy("D:/Movies/a", 0, MediaSource::Unknown, ("", 0));
        let hd = lab.copy("D:/Movies/b", 1920, MediaSource::WebDl, ("AAC", 6));
        let uhd = lab.copy("D:/Movies/c", 3840, MediaSource::Bluray, ("AAC", 6));

        let members = lab.service.listing().unwrap();
        let order: Vec<(ItemId, u32)> = members.iter().map(|m| (m.id, m.rank)).collect();
        assert_eq!(order, vec![(uhd, 0), (hd, 1), (unread, 2)]);
        assert!(members.iter().all(|m| m.group == unread));

        lab.service.keep_all(&[unread, hd, uhd]).unwrap();
        assert!(lab.service.listing().unwrap().is_empty());
    }

    #[test]
    fn a_movie_without_copies_has_no_group_and_no_page() {
        let lab = Lab::new();
        let a = lab.copy("D:/Movies/a", 1920, MediaSource::WebDl, ("AAC", 6));

        assert_eq!(lab.service.group_of(a).unwrap(), None);
        assert_eq!(lab.service.compare(a).unwrap(), None);
        assert_eq!(lab.service.counts().unwrap(), 0);
    }
}
