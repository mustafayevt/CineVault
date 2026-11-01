//! The copies service: what the Library, the Movie page and the Compare copies page ask for.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

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
    /// The copy's video may be the same file as another copy's (see [`Comparison::same_file`]).
    pub same_file: bool,
}

/// The Compare copies page of one movie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    /// The movie's own copy first, then the others in id order.
    pub copies: Vec<CopyRow>,
    /// The "keep both" decision covering the group, when the user made one.
    pub kept: Option<KeptSetId>,
    /// Every copy shows the same Resolution, HDR, Audio and Source, their lengths are within
    /// [`SAME_LENGTH_MINUTES`] of each other, and no two may be one file.
    pub look_same: bool,
    /// Two of the copies may be one file reached through two library folders (a network share
    /// of a local folder, a mapped drive, a link): deleting one would delete the other, so the
    /// page offers no "Keep this one".
    pub same_file: bool,
}

/// Why "Keep this one" may not delete the other copies. Nothing was deleted.
#[derive(Debug, thiserror::Error)]
pub enum KeepCopyError {
    #[error("Choose the copy to keep and at least one other copy.")]
    NothingToDelete,
    #[error(
        "The copy to keep is no longer in the library or cannot be reached, so nothing was deleted."
    )]
    KeepGone,
    #[error(
        "The copies changed since this page was opened, so nothing was deleted. Look at them again."
    )]
    Changed,
    #[error(
        "These may be the same file reached through two library folders. CineVault won't delete either from here; check the folders under Settings › Library folders."
    )]
    SameFile,
    #[error(transparent)]
    Db(#[from] DbError),
}

impl KeepCopyError {
    /// A stable code for the UI.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NothingToDelete => "nothing_to_delete",
            Self::KeepGone => "keep_gone",
            Self::Changed => "copies_changed",
            Self::SameFile => "same_file",
            Self::Db(_) => "storage",
        }
    }
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

        let shared = may_share_a_file(&items);
        let same_file = shared.iter().any(|shared| *shared);
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
                    same_file: shared[index],
                }
            })
            .collect();
        let look_same = !same_file && look_alike(&copies);
        Ok(Some(Comparison {
            copies,
            kept: group.kept,
            look_same,
            same_file,
        }))
    }

    /// "Keep this one", checked against the library as it is now, before anything is deleted:
    /// `keep` must still be a copy that has its files, every one of `others` must be a copy in
    /// its group, and no two copies of the group may be one file. Returns the copies the delete
    /// may take (`others` without repeats); the caller deletes exactly those. Deletes nothing
    /// itself. Blocking.
    ///
    /// # Errors
    /// The [`KeepCopyError`] that says why nothing may be deleted, or the library cannot be read.
    pub fn keep_copy(&self, keep: ItemId, others: &[ItemId]) -> Result<Vec<ItemId>, KeepCopyError> {
        let mut confirmed: Vec<ItemId> = Vec::with_capacity(others.len());
        for id in others {
            if !confirmed.contains(id) {
                confirmed.push(*id);
            }
        }
        if confirmed.is_empty() || confirmed.contains(&keep) {
            return Err(KeepCopyError::NothingToDelete);
        }
        let group = self.group_of(keep)?.ok_or(KeepCopyError::KeepGone)?;
        if !confirmed.iter().all(|id| group.members.contains(id)) {
            return Err(KeepCopyError::Changed);
        }
        let members = group.members;
        let items = self.db.read(move |conn| {
            let mut items = Vec::with_capacity(members.len());
            for id in &members {
                items.extend(cv_db::repo::get_item(conn, *id)?);
            }
            Ok(items)
        })?;
        if may_share_a_file(&items).contains(&true) {
            return Err(KeepCopyError::SameFile);
        }
        tracing::info!(
            keep = keep.0,
            others = confirmed.len(),
            "keep this one checked"
        );
        Ok(confirmed)
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

/// How far apart the lengths of copies that "look the same" may be.
pub const SAME_LENGTH_MINUTES: u32 = 3;

/// Every copy has read media info with the same Resolution, HDR, Audio and Source, and a known
/// length within [`SAME_LENGTH_MINUTES`] of the others: two films of one title and year can share
/// everything else.
fn look_alike(copies: &[CopyRow]) -> bool {
    let Some(media) = copies
        .iter()
        .map(|c| c.media.as_ref())
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let lengths = || media.iter().map(|m| m.duration_minutes);
    let (Some(shortest), Some(longest)) = (lengths().min(), lengths().max()) else {
        return false;
    };
    shortest > 0
        && longest - shortest <= SAME_LENGTH_MINUTES
        && copies.windows(2).zip(media.windows(2)).all(|(pair, m)| {
            (&m[0].resolution, &m[0].hdr_formats, &m[0].audio)
                == (&m[1].resolution, &m[1].hdr_formats, &m[1].audio)
                && pair[0].source == pair[1].source
        })
}

/// For each copy, whether its main video may be the same file as another copy's. A copy without
/// a video it can name cannot be told apart, so it counts as maybe the same.
fn may_share_a_file(items: &[Item]) -> Vec<bool> {
    let videos: Vec<_> = items
        .iter()
        .map(|item| main_video(item).map(cv_domain::MediaFile::path))
        .collect();
    let mut shared = vec![false; videos.len()];
    for a in 0..videos.len() {
        for b in a + 1..videos.len() {
            let same = match (&videos[a], &videos[b]) {
                (Some(first), Some(second)) => may_be_same_file(first, second),
                _ => true,
            };
            if same {
                shared[a] = true;
                shared[b] = true;
            }
        }
    }
    shared
}

/// Whether `a` and `b` may be one file reached two ways, such as a file under `D:\Cinema` and
/// the same file under the network share of that folder. The file system answers first; when it
/// does not say so, the same file name, size and modified time on both paths still count as
/// maybe the same, because a share does not always report the identity of the disk behind it. A
/// file that cannot be read cannot be told apart either.
fn may_be_same_file(a: &Path, b: &Path) -> bool {
    if same_file::is_same_file(a, b).unwrap_or(false) {
        return true;
    }
    let (Ok(first), Ok(second)) = (std::fs::metadata(a), std::fs::metadata(b)) else {
        return true;
    };
    let name = |path: &Path| path.file_name().map(|n| n.to_string_lossy().to_lowercase());
    // Some file systems keep modified times to two seconds only.
    let close_in_time = match (first.modified(), second.modified()) {
        (Ok(x), Ok(y)) => {
            x.duration_since(y).unwrap_or_else(|e| e.duration()) <= Duration::from_secs(2)
        }
        _ => true,
    };
    name(a) == name(b) && first.len() == second.len() && close_in_time
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
        dir: tempfile::TempDir,
        db: Arc<Database>,
        service: Arc<CopiesService>,
    }

    impl Lab {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
            let service = CopiesService::new(Arc::clone(&db));
            Self { dir, db, service }
        }

        /// A copy of Example Movie (2020) in its own folder `name` of the media folder, with a
        /// tiny video named after the folder; `width` 0 leaves its media info unread.
        fn copy(&self, name: &str, width: u32, source: MediaSource, audio: (&str, u32)) -> ItemId {
            let folder = self.dir.path().join("media").join(name);
            self.copy_in(
                &folder,
                &format!("Example Movie {name}.mkv"),
                width,
                source,
                audio,
            )
        }

        /// A copy whose video is `folder/file`; the file is written when it is not there yet.
        fn copy_in(
            &self,
            folder: &std::path::Path,
            file: &str,
            width: u32,
            source: MediaSource,
            audio: (&str, u32),
        ) -> ItemId {
            std::fs::create_dir_all(folder).unwrap();
            if !folder.join(file).exists() {
                std::fs::write(folder.join(file), file).unwrap();
            }
            let folder = folder.to_string_lossy().into_owned();
            let mut item = Item::movie("Example Movie");
            item.core.year = 2020;
            item.core.path = folder.clone();
            item.core.ids.set("tmdb", "1");
            if let ItemDetail::Movie(movie) = &mut item.detail {
                movie.media_source = source;
            }
            let mut video = MediaFile::new(MediaFileType::Video, &folder, file);
            video.size_bytes = u64::from(width) * 1000;
            if width > 0 {
                video.container_format = "Matroska".into();
                video.width = width;
                video.height = width * 9 / 16;
                video.video_bitrate_kbps = width * 5;
                video.duration_secs = 6000;
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

    /// Makes `link` point at the folder `target`: a junction on Windows (no rights needed), a
    /// symbolic link elsewhere. False when the system will not.
    fn folder_link(target: &std::path::Path, link: &std::path::Path) -> bool {
        #[cfg(windows)]
        {
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .is_ok_and(|o| o.status.success())
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
    }

    #[test]
    fn the_compare_rows_put_the_opened_copy_first_and_mark_the_best() {
        let lab = Lab::new();
        let web = lab.copy("dl-1", 1920, MediaSource::WebDl, ("AAC", 6));
        let disc = lab.copy("dl-2", 3840, MediaSource::Bluray, ("TrueHD Atmos", 8));

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
        assert!(second.folder.ends_with("dl-1"));
        assert!(!page.same_file && !first.same_file);
        assert!(!page.look_same);
        assert_eq!(page.kept, None);
    }

    #[test]
    fn an_unread_copy_shows_not_read_yet_and_nothing_is_marked() {
        let lab = Lab::new();
        let read = lab.copy("a", 3840, MediaSource::Bluray, ("AAC", 6));
        let unread = lab.copy("b", 0, MediaSource::Unknown, ("", 0));

        let page = lab.service.compare(read).unwrap().unwrap();

        assert!(page.copies.iter().all(|c| !c.best));
        assert_eq!(page.copies[1].id, unread);
        assert_eq!(page.copies[1].media, None);
        assert_eq!(page.copies[1].source, "");
    }

    #[test]
    fn identical_copies_look_the_same() {
        let lab = Lab::new();
        let a = lab.copy("a", 1920, MediaSource::WebDl, ("AAC", 6));
        lab.copy("b", 1920, MediaSource::WebDl, ("AAC", 6));

        let page = lab.service.compare(a).unwrap().unwrap();

        assert!(page.look_same);
        assert!(page.copies.iter().all(|c| !c.best), "a tie marks nothing");
    }

    #[test]
    fn keep_all_closes_the_group_and_ask_again_opens_it() {
        let lab = Lab::new();
        let a = lab.copy("a", 1920, MediaSource::WebDl, ("AAC", 6));
        let b = lab.copy("b", 3840, MediaSource::Bluray, ("AAC", 6));
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
        let unread = lab.copy("a", 0, MediaSource::Unknown, ("", 0));
        let hd = lab.copy("b", 1920, MediaSource::WebDl, ("AAC", 6));
        let uhd = lab.copy("c", 3840, MediaSource::Bluray, ("AAC", 6));

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
        let a = lab.copy("a", 1920, MediaSource::WebDl, ("AAC", 6));

        assert_eq!(lab.service.group_of(a).unwrap(), None);
        assert_eq!(lab.service.compare(a).unwrap(), None);
        assert_eq!(lab.service.counts().unwrap(), 0);
    }

    #[test]
    fn one_file_through_two_library_folders_is_marked_and_does_not_look_the_same() {
        let lab = Lab::new();
        let real = lab.dir.path().join("Cinema");
        let share = lab.dir.path().join("D-Cinema");
        std::fs::create_dir_all(&real).unwrap();
        if !folder_link(&real, &share) {
            return;
        }
        let file = "Example Movie (2020).mkv";
        let local = lab.copy_in(
            &real.join("Example Movie"),
            file,
            1920,
            MediaSource::WebDl,
            ("AAC", 6),
        );
        lab.copy_in(
            &share.join("Example Movie"),
            file,
            1920,
            MediaSource::WebDl,
            ("AAC", 6),
        );

        let page = lab.service.compare(local).unwrap().unwrap();

        assert!(page.same_file);
        assert!(page.copies.iter().all(|c| c.same_file));
        assert!(!page.look_same, "never suggest keeping one of one file");
    }

    #[test]
    fn same_name_size_and_time_on_two_paths_may_be_the_same_file() {
        let lab = Lab::new();
        let file = "Example Movie (2020).mkv";
        let a = lab.copy_in(
            &lab.dir.path().join("one"),
            file,
            1920,
            MediaSource::WebDl,
            ("AAC", 6),
        );
        let b = lab.copy_in(
            &lab.dir.path().join("two"),
            file,
            3840,
            MediaSource::Bluray,
            ("AAC", 6),
        );
        let when = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
        for folder in ["one", "two"] {
            let path = lab.dir.path().join(folder).join(file);
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(when)
                .unwrap();
        }

        let page = lab.service.compare(a).unwrap().unwrap();
        assert!(page.same_file);

        // Another modified time tells them apart.
        let path = lab.dir.path().join("two").join(file);
        let later = when + Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(!lab.service.compare(b).unwrap().unwrap().same_file);
    }

    #[test]
    fn a_copy_whose_video_cannot_be_read_cannot_be_told_apart() {
        let lab = Lab::new();
        let a = lab.copy("a", 1920, MediaSource::WebDl, ("AAC", 6));
        lab.copy("b", 3840, MediaSource::Bluray, ("AAC", 6));
        std::fs::remove_file(lab.dir.path().join("media/b/Example Movie b.mkv")).unwrap();

        assert!(lab.service.compare(a).unwrap().unwrap().same_file);
    }

    fn set(lab: &Lab, sql: &'static str, id: ItemId) {
        lab.db
            .write(move |tx| Ok(tx.execute(sql, [id.0]).map(|_| ())?))
            .unwrap();
    }

    #[test]
    fn keep_this_one_confirms_the_other_copies_of_the_group() {
        let lab = Lab::new();
        let a = lab.copy("a", 1920, MediaSource::WebDl, ("AAC", 6));
        let b = lab.copy("b", 3840, MediaSource::Bluray, ("AAC", 6));
        let c = lab.copy("c", 1280, MediaSource::WebDl, ("AAC", 2));

        assert_eq!(lab.service.keep_copy(b, &[a, c, a]).unwrap(), vec![a, c]);
        // A page opened before a third copy was found deletes only what it showed.
        assert_eq!(lab.service.keep_copy(b, &[a]).unwrap(), vec![a]);
        // A kept group still offers Keep this one.
        lab.service.keep_all(&[a, b, c]).unwrap();
        assert_eq!(lab.service.keep_copy(a, &[b]).unwrap(), vec![b]);
    }

    #[test]
    fn keep_this_one_refuses_what_it_cannot_be_sure_of() {
        let lab = Lab::new();
        let a = lab.copy("a", 1920, MediaSource::WebDl, ("AAC", 6));
        let b = lab.copy("b", 3840, MediaSource::Bluray, ("AAC", 6));
        let c = lab.copy("c", 1280, MediaSource::WebDl, ("AAC", 2));
        let mut other = Item::movie("Another Example");
        other.core.ids.set("tmdb", "2");
        let other = lab.db.write(move |tx| insert_item(tx, &other)).unwrap();
        let code = |result: Result<Vec<ItemId>, KeepCopyError>| result.unwrap_err().code();

        assert_eq!(code(lab.service.keep_copy(a, &[])), "nothing_to_delete");
        assert_eq!(code(lab.service.keep_copy(a, &[a, b])), "nothing_to_delete");
        // A movie that is not a copy of this film.
        assert_eq!(
            code(lab.service.keep_copy(a, &[b, other])),
            "copies_changed"
        );

        // Another window already deleted the copy this one keeps: deleting the rest would leave
        // no copy at all.
        set(&lab, "UPDATE item SET trashed_entry = 7 WHERE id = ?1", a);
        assert_eq!(code(lab.service.keep_copy(a, &[b, c])), "keep_gone");
        // ... or a copy this page still shows.
        assert_eq!(code(lab.service.keep_copy(b, &[a, c])), "copies_changed");
        set(
            &lab,
            "UPDATE item SET trashed_entry = NULL WHERE id = ?1",
            a,
        );

        // The kept copy's drive is offline.
        set(
            &lab,
            "UPDATE item SET availability = 'offline' WHERE id = ?1",
            b,
        );
        assert_eq!(code(lab.service.keep_copy(b, &[a, c])), "keep_gone");
        set(
            &lab,
            "UPDATE item SET availability = 'online' WHERE id = ?1",
            b,
        );

        // Two of the copies may be one file.
        std::fs::remove_file(lab.dir.path().join("media/c/Example Movie c.mkv")).unwrap();
        assert_eq!(code(lab.service.keep_copy(a, &[b])), "same_file");
    }

    #[test]
    fn copies_whose_lengths_differ_do_not_look_the_same() {
        let lab = Lab::new();
        let a = lab.copy("a", 1920, MediaSource::WebDl, ("AAC", 6));
        let b = lab.copy("b", 1920, MediaSource::WebDl, ("AAC", 6));
        let length = |id: ItemId, secs: u32| {
            lab.db
                .write(move |tx| {
                    Ok(tx
                        .execute(
                            "UPDATE media_file SET duration_secs = ?2 WHERE item_id = ?1",
                            rusqlite::params![id.0, secs],
                        )
                        .map(|_| ())?)
                })
                .unwrap();
        };

        // Three minutes apart still look the same (a different cut of the credits) ...
        length(b, 6000 + 180);
        assert!(lab.service.compare(a).unwrap().unwrap().look_same);
        // ... more does not, and neither does an unknown length.
        length(b, 6000 + 240);
        assert!(!lab.service.compare(a).unwrap().unwrap().look_same);
        length(b, 0);
        assert!(!lab.service.compare(a).unwrap().unwrap().look_same);
    }
}
