//! Keeping NFO files current: every change of a title's data is written to its NFO.
//!
//! The writes that change what an NFO holds announce it on the [`ChangeHub`] (a scrape, an edit,
//! a bulk edit or a lock, ratings, artwork, media info, subtitles, a rename, watched state, a
//! collection). [`NfoFollower`] listens there and rewrites the NFOs of the titles that changed
//! through the one write path ([`NfoService::rewrite`]: only when the content differs, written
//! atomically, with an undo record), so the files on disk match the library whatever wrote the
//! change. Many writers also rewrite the NFO themselves right away; the follower's pass is then
//! a no-op for them, since an unchanged NFO is never written again.
//!
//! The rules:
//!
//! * Only titles CineVault scraped (a scraper wrote their details): an NFO that came with an
//!   unscraped folder is left as the user had it. A season or an episode counts as its show.
//! * Nothing when "Write NFO files in automatic steps" is off.
//! * Bursts are coalesced: the titles of every announcement that arrives while a pass waits or
//!   runs are written in the next pass, each title once.

use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cv_db::Database;
use cv_domain::ItemId;
use rusqlite::OptionalExtension;

use super::service::{NfoScope, NfoService};
use super::settings::NfoSettings;
use crate::app_settings::SettingsService;
use crate::changes::{ChangeHub, ChangePart, ItemsChanged};

/// How long the follower waits for more changes before it writes.
const SETTLE: Duration = Duration::from_millis(300);

/// The parts of a title whose change can change its NFO.
const NFO_PARTS: [ChangePart; 4] = [
    ChangePart::Details,
    ChangePart::Ratings,
    ChangePart::Artwork,
    ChangePart::Files,
];

/// Whether an announcement can change an NFO.
#[must_use]
pub fn touches_nfo(change: &ItemsChanged) -> bool {
    change.parts.iter().any(|part| NFO_PARTS.contains(part))
}

/// Rewrites the NFOs of the titles the [`ChangeHub`] reports as changed (see the module docs).
pub struct NfoFollower {
    sender: Mutex<Sender<Vec<ItemId>>>,
}

impl NfoFollower {
    /// Starts the follower: a worker thread, and a listener on `changes`.
    pub fn start(
        db: Arc<Database>,
        nfo: Arc<NfoService>,
        settings: Arc<SettingsService>,
        changes: &Arc<ChangeHub>,
    ) -> Arc<Self> {
        Self::start_with(
            db,
            nfo,
            Box::new(move || NfoSettings::from_values(&settings.values())),
            changes,
        )
    }

    /// [`Self::start`] with the NFO settings read by `settings` before each pass.
    pub fn start_with(
        db: Arc<Database>,
        nfo: Arc<NfoService>,
        settings: Box<dyn Fn() -> NfoSettings + Send>,
        changes: &Arc<ChangeHub>,
    ) -> Arc<Self> {
        let (sender, receiver) = channel::<Vec<ItemId>>();
        let worker = Worker { db, nfo, settings };
        let _ = std::thread::Builder::new()
            .name("nfo-follower".into())
            .spawn(move || worker.run(&receiver));
        let follower = Arc::new(Self {
            sender: Mutex::new(sender),
        });
        let listener = Arc::clone(&follower);
        changes.subscribe(Arc::new(move |change: &ItemsChanged| {
            if touches_nfo(change) {
                listener.note(&change.items);
            }
        }));
        follower
    }

    /// Queues `items` for the next pass.
    pub fn note(&self, items: &[ItemId]) {
        if items.is_empty() {
            return;
        }
        let _ = self
            .sender
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(items.to_vec());
    }
}

struct Worker {
    db: Arc<Database>,
    nfo: Arc<NfoService>,
    settings: Box<dyn Fn() -> NfoSettings + Send>,
}

impl Worker {
    fn run(&self, receiver: &Receiver<Vec<ItemId>>) {
        while let Ok(first) = receiver.recv() {
            let mut items: BTreeSet<ItemId> = first.into_iter().collect();
            // Let the burst settle, then take everything that arrived meanwhile.
            std::thread::sleep(SETTLE);
            while let Ok(more) = receiver.try_recv() {
                items.extend(more);
            }
            self.pass(&items.into_iter().collect::<Vec<_>>());
        }
    }

    fn pass(&self, items: &[ItemId]) {
        let settings = (self.settings)();
        let owned = match self.db.read(|c| owned_by_cinevault(c, items)) {
            Ok(owned) => owned,
            Err(error) => {
                tracing::warn!(%error, "the titles to write NFOs for could not be read");
                return;
            }
        };
        if owned.is_empty() {
            return;
        }
        match self.nfo.rewrite(&NfoScope::Items(owned), &settings) {
            Ok(report) if report.totals.failed > 0 => {
                tracing::warn!(failed = report.totals.failed, "some NFOs were not written");
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "the NFOs of changed titles were not written"),
        }
    }
}

/// Which of `items` have NFOs that are CineVault's to keep current: titles a scraper wrote the
/// details of. A season or an episode counts as its show; ids the library does not know are
/// left out.
///
/// # Errors
/// The database fails.
pub fn owned_by_cinevault(
    conn: &rusqlite::Connection,
    items: &[ItemId],
) -> cv_db::DbResult<Vec<ItemId>> {
    let mut stmt = conn.prepare_cached(
        "SELECT CASE WHEN i.kind IN ('season', 'episode') \
                     THEN COALESCE(p.last_scraper_id, '') ELSE i.last_scraper_id END \
         FROM item i LEFT JOIN item p ON p.id = i.parent_id WHERE i.id = ?1",
    )?;
    let mut out = Vec::new();
    for id in items {
        let scraper: Option<String> = stmt.query_row([id.0], |r| r.get(0)).optional()?;
        if scraper.is_some_and(|s| !s.trim().is_empty()) {
            out.push(*id);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use cv_db::repo::{get_item, insert_item, update_item};
    use cv_domain::{Item, MediaFile, MediaFileType};

    use super::*;

    struct Lab {
        root: tempfile::TempDir,
        db: Arc<Database>,
        nfo: Arc<NfoService>,
        hub: Arc<ChangeHub>,
    }

    impl Lab {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let db = Arc::new(Database::open(root.path().join("library.db")).unwrap());
            let nfo = Arc::new(NfoService::new(Arc::clone(&db), root.path().join("undo")));
            let hub = ChangeHub::new(Duration::ZERO);
            NfoFollower::start_with(
                Arc::clone(&db),
                Arc::clone(&nfo),
                Box::new(NfoSettings::default),
                &hub,
            );
            Self { root, db, nfo, hub }
        }

        /// A movie in its own folder; `scraped` gives it a scraper.
        fn movie(&self, title: &str, scraped: bool) -> (ItemId, PathBuf) {
            let folder = self.root.path().join(title);
            fs::create_dir_all(&folder).unwrap();
            let video = format!("{title}.mkv");
            fs::write(folder.join(&video), b"v").unwrap();
            let mut item = Item::movie(title);
            item.core.path = folder.to_string_lossy().into_owned();
            item.core.ids.set("tmdb", "949");
            if scraped {
                item.core.last_scraper_id = "tmdb".into();
            }
            item.core.media_files.push(MediaFile::new(
                MediaFileType::Video,
                folder.to_string_lossy(),
                video,
            ));
            let id = self.db.write(move |tx| insert_item(tx, &item)).unwrap();
            (id, folder.join(format!("{title}.nfo")))
        }

        fn set_plot(&self, id: ItemId, plot: &str) {
            let plot = plot.to_owned();
            self.db
                .write(move |tx| {
                    let mut item = get_item(tx, id)?.unwrap();
                    item.core.plot = plot;
                    update_item(tx, &item)
                })
                .unwrap();
        }
    }

    fn wait_for(path: &Path, text: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if fs::read_to_string(path).is_ok_and(|t| t.contains(text)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn a_change_of_a_scraped_title_is_written_to_its_nfo() {
        let lab = Lab::new();
        let (id, nfo) = lab.movie("Heat", true);
        lab.nfo
            .rewrite(&NfoScope::Items(vec![id]), &NfoSettings::default())
            .unwrap();
        lab.set_plot(id, "A new plot from an edit.");
        lab.hub.publish(&[id], &[ChangePart::Details]);
        assert!(wait_for(&nfo, "<plot>A new plot from an edit.</plot>"));

        // Ratings, artwork, files and subtitles count too.
        lab.set_plot(id, "Changed with the files.");
        lab.hub.publish(&[id], &[ChangePart::Files]);
        assert!(wait_for(&nfo, "<plot>Changed with the files.</plot>"));
    }

    #[test]
    fn an_unscraped_title_keeps_its_own_nfo() {
        let lab = Lab::new();
        let (id, nfo) = lab.movie("Home Video", false);
        fs::write(&nfo, "<movie><title>Home Video</title></movie>").unwrap();
        lab.set_plot(id, "Not for the file.");
        lab.hub.publish(&[id], &[ChangePart::Details]);
        // A scraped title published after it is written, so the pass that would have taken
        // the first one is over.
        let (other, other_nfo) = lab.movie("Heat", true);
        lab.hub.publish(&[other], &[ChangePart::Details]);
        assert!(wait_for(&other_nfo, "<title>Heat</title>"));
        assert_eq!(
            fs::read_to_string(&nfo).unwrap(),
            "<movie><title>Home Video</title></movie>"
        );
    }

    /// Waits until the NFO on disk is what Rewrite NFO renders from the library now.
    fn in_sync(lab: &Lab, id: ItemId) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let preview = lab
                .nfo
                .preview_movie(id, &NfoSettings::default())
                .unwrap()
                .unwrap();
            if preview.sync == super::super::NfoSync::InSync {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// Each kind of change, as the write that makes it announces it, ends with the NFO on disk
    /// matching the library: an edit, a lock, ratings, artwork, media info, a subtitle file,
    /// watched state and a collection.
    #[test]
    fn every_kind_of_change_ends_with_the_nfo_matching_the_library() {
        use cv_domain::{AudioStream, Rating};
        let lab = Lab::new();
        let (id, _) = lab.movie("Heat", true);
        let set = lab
            .db
            .write(|tx| insert_item(tx, &Item::set("Heat Collection")))
            .unwrap();
        lab.nfo
            .rewrite(&NfoScope::Items(vec![id]), &NfoSettings::default())
            .unwrap();
        assert!(in_sync(&lab, id));

        type Change = Box<dyn Fn(&mut Item)>;
        let changes: Vec<(&str, ChangePart, Change)> = vec![
            (
                "edit",
                ChangePart::Details,
                Box::new(|i| i.core.plot = "Edited.".into()),
            ),
            (
                "ratings",
                ChangePart::Ratings,
                Box::new(|i| {
                    i.core.ratings.insert(Rating::new("imdb", 8.3, 700_000, 10));
                }),
            ),
            (
                "artwork",
                ChangePart::Artwork,
                Box::new(|i| {
                    i.core
                        .artwork_urls
                        .insert(MediaFileType::Poster, "https://img.test/p.jpg".into());
                }),
            ),
            (
                "media info",
                ChangePart::Files,
                Box::new(|i| {
                    let video = i
                        .core
                        .media_files
                        .iter_mut()
                        .find(|f| f.kind == MediaFileType::Video)
                        .unwrap();
                    video.video_codec = "HEVC".into();
                    video.width = 3840;
                    video.height = 2160;
                    video.audio_streams = vec![AudioStream {
                        codec: "TrueHD Atmos".into(),
                        language: "en".into(),
                        channels: 8,
                        ..AudioStream::default()
                    }];
                }),
            ),
            (
                "subtitle file",
                ChangePart::Files,
                Box::new(|i| {
                    let dir = i.core.path.clone();
                    i.core.media_files.push(MediaFile::new(
                        MediaFileType::Subtitle,
                        dir,
                        "Heat.deu.forced.srt",
                    ));
                }),
            ),
            (
                "watched",
                ChangePart::Details,
                Box::new(|i| {
                    if let cv_domain::ItemDetail::Movie(m) = &mut i.detail {
                        m.watched = true;
                        m.play_count = 2;
                    }
                }),
            ),
            (
                "collection",
                ChangePart::Details,
                Box::new(move |i| i.parent_id = Some(set)),
            ),
        ];
        for (name, part, change) in changes {
            let mut item = lab.db.read(|c| get_item(c, id)).unwrap().unwrap();
            change(&mut item);
            lab.db.write(move |tx| update_item(tx, &item)).unwrap();
            let preview = lab
                .nfo
                .preview_movie(id, &NfoSettings::default())
                .unwrap()
                .unwrap();
            assert_ne!(
                preview.sync,
                super::super::NfoSync::InSync,
                "{name} changes the NFO"
            );
            lab.hub.publish(&[id], &[part]);
            assert!(in_sync(&lab, id), "the NFO follows the {name} change");
        }

        // A field lock is listed in the NFO.
        lab.db
            .write(move |tx| cv_db::repo::set_field_lock(tx, id, cv_domain::MetaField::Plot, true))
            .unwrap();
        lab.hub.publish(&[id], &[ChangePart::Details]);
        assert!(in_sync(&lab, id), "the NFO follows a lock");
        let text = fs::read_to_string(lab.root.path().join("Heat").join("Heat.nfo")).unwrap();
        assert!(text.contains("<cv_locked>plot</cv_locked>"), "{text}");
    }

    #[test]
    fn only_parts_an_nfo_holds_start_a_pass() {
        let stage = ItemsChanged {
            items: vec![ItemId(1)],
            parts: vec![ChangePart::Stage],
        };
        assert!(!touches_nfo(&stage));
        let ratings = ItemsChanged {
            items: vec![ItemId(1)],
            parts: vec![ChangePart::Stage, ChangePart::Ratings],
        };
        assert!(touches_nfo(&ratings));
    }
}
