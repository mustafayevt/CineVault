//! Tests of watched state and the NEW flag over a real database and folders.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::{get_watch_state, insert_item, set_new};
use cv_domain::{EpisodeNumber, Item, ItemDetail, ItemId, ItemKind, MediaFile, MediaFileType};

use super::{NewFlagService, WatchService};
use crate::nfo::{NfoService, NfoSettings};

struct Lab {
    root: tempfile::TempDir,
    db: Arc<Database>,
    nfo: Arc<NfoService>,
}

impl Lab {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(root.path().join("library.db")).unwrap());
        let nfo = Arc::new(NfoService::new(Arc::clone(&db), root.path().join("undo")));
        Self { root, db, nfo }
    }

    fn watch(&self) -> WatchService {
        WatchService::new(Arc::clone(&self.db), Arc::clone(&self.nfo))
    }

    fn flags(&self) -> NewFlagService {
        NewFlagService::new(Arc::clone(&self.db))
    }

    fn insert(&self, item: Item) -> ItemId {
        self.db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    /// A movie "Film N (2000)" in its own folder with a video file and the given tags.
    fn movie(&self, n: usize, tags: &[&str]) -> (ItemId, PathBuf) {
        let name = format!("Film {n}");
        let folder = self
            .root
            .path()
            .join("Movies")
            .join(format!("{name} (2000)"));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(format!("{name} (2000).mkv")), b"video").unwrap();
        let mut item = Item::movie(name.clone());
        item.core.year = 2000;
        item.core.path = folder.to_string_lossy().into_owned();
        item.core.tags = tags
            .iter()
            .map(|t| (*t).to_owned())
            .collect::<Vec<_>>()
            .into();
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            folder.to_string_lossy(),
            format!("{name} (2000).mkv"),
        ));
        (self.insert(item), folder)
    }

    /// A show with two seasons: season 1 has episodes 1-2, season 2 has episodes 1-3.
    fn show(&self) -> (ItemId, [ItemId; 2], Vec<ItemId>) {
        let show = self.insert(Item::show("Show"));
        let seasons = [
            self.insert(Item::season(show, 1)),
            self.insert(Item::season(show, 2)),
        ];
        let mut episodes = Vec::new();
        for (season, number) in [(1, 1), (1, 2), (2, 1), (2, 2), (2, 3)] {
            let mut episode = Item::episode(show, format!("S{season}E{number}"));
            if let ItemDetail::Episode(fields) = &mut episode.detail {
                fields.numbers.set(EpisodeNumber::aired(season, number));
            }
            episodes.push(self.insert(episode));
        }
        (show, seasons, episodes)
    }

    fn state(&self, id: ItemId) -> (bool, u32, Option<i64>) {
        let state = self.db.read(|c| get_watch_state(c, id)).unwrap().unwrap();
        (state.watched, state.play_count, state.last_watched_ms)
    }
}

fn nfo_of(n: usize, folder: &std::path::Path) -> String {
    fs::read_to_string(folder.join(format!("Film {n} (2000).nfo"))).unwrap_or_default()
}

fn settings() -> NfoSettings {
    NfoSettings::default()
}

// ---- watched --------------------------------------------------------------------------------

#[test]
fn marking_a_movie_watched_sets_the_count_and_date_and_writes_the_nfo() {
    let lab = Lab::new();
    let (id, folder) = lab.movie(1, &[]);
    let report = lab.watch().set_watched(&[id], true, &settings()).unwrap();
    assert_eq!((report.covered, report.changed()), (1, 1));
    let (watched, plays, last) = lab.state(id);
    assert!(watched && plays >= 1 && last.is_some());
    let nfo = nfo_of(1, &folder);
    assert!(nfo.contains("<watched>true</watched>"), "{nfo}");
    assert!(nfo.contains("<playcount>1</playcount>"), "{nfo}");

    let again = lab.watch().set_watched(&[id], true, &settings()).unwrap();
    assert_eq!(again.changed(), 0, "already watched: nothing changes");

    let off = lab.watch().set_watched(&[id], false, &settings()).unwrap();
    assert_eq!(off.changed(), 1);
    assert_eq!(lab.state(id), (false, 0, None));
    assert!(nfo_of(1, &folder).contains("<playcount>0</playcount>"));
}

#[test]
fn undo_puts_back_the_play_count_a_title_had() {
    let lab = Lab::new();
    let (id, _) = lab.movie(1, &[]);
    let service = lab.watch();
    service.set_watched(&[id], true, &settings()).unwrap();
    lab.db
        .write(move |tx| {
            let mut state = get_watch_state(tx, id)?.unwrap();
            state.play_count = 3;
            cv_db::repo::set_watch_state(tx, &state)
        })
        .unwrap();
    let before = lab.state(id);

    let off = service.set_watched(&[id], false, &settings()).unwrap();
    assert_eq!(lab.state(id), (false, 0, None));
    service.restore(&off.previous, &settings()).unwrap();
    assert_eq!(lab.state(id), before);
    assert_eq!(before.1, 3);
}

#[test]
fn marking_season_two_watched_watches_exactly_its_episodes() {
    let lab = Lab::new();
    let (_, seasons, episodes) = lab.show();
    let report = lab
        .watch()
        .set_watched(&[seasons[1]], true, &settings())
        .unwrap();
    assert_eq!((report.covered, report.changed()), (3, 3));
    for (index, id) in episodes.iter().enumerate() {
        let (watched, plays, last) = lab.state(*id);
        if index >= 2 {
            assert!(watched && plays >= 1 && last.is_some(), "episode {index}");
        } else {
            assert_eq!((watched, plays, last), (false, 0, None), "episode {index}");
        }
    }
}

#[test]
fn marking_a_show_watched_covers_every_episode_and_undo_reverts_only_those() {
    let lab = Lab::new();
    let (show, seasons, episodes) = lab.show();
    let service = lab.watch();
    service
        .set_watched(&[seasons[0]], true, &settings())
        .unwrap();
    let report = service.set_watched(&[show], true, &settings()).unwrap();
    assert_eq!(report.covered, 5);
    assert_eq!(report.changed(), 3, "season 1 was already watched");
    assert!(episodes.iter().all(|id| lab.state(*id).0));
    service.restore(&report.previous, &settings()).unwrap();
    let watched: Vec<bool> = episodes.iter().map(|id| lab.state(*id).0).collect();
    assert_eq!(watched, [true, true, false, false, false]);
}

#[test]
fn toggling_a_season_watches_it_unless_it_is_already_all_watched() {
    let lab = Lab::new();
    let (_, seasons, episodes) = lab.show();
    let service = lab.watch();
    service
        .set_watched(&episodes[2..3], true, &settings())
        .unwrap();
    let (now, report) = service.toggle(&[seasons[1]], &settings()).unwrap();
    assert!(
        now,
        "one episode was watched, so the season becomes watched"
    );
    assert_eq!(report.changed(), 2);
    let (now, report) = service.toggle(&[seasons[1]], &settings()).unwrap();
    assert!(
        !now,
        "every episode was watched, so the season is unwatched"
    );
    assert_eq!(report.changed(), 3);
    assert!(episodes.iter().all(|id| !lab.state(*id).0));
}

// ---- new flag -------------------------------------------------------------------------------

#[test]
fn clear_new_with_five_selected_clears_those_five_and_undo_flags_them_again() {
    let lab = Lab::new();
    let ids: Vec<ItemId> = (1..=8).map(|n| lab.movie(n, &[]).0).collect();
    let all = ids.clone();
    lab.db.write(move |tx| set_new(tx, &all, true)).unwrap();
    let service = lab.flags();

    let report = service.clear(&ids[..5]).unwrap();
    assert_eq!(report.changed, ids[..5].to_vec());
    assert_eq!(service.count(None).unwrap(), 3);
    assert!(service.clear(&ids[..5]).unwrap().changed.is_empty());

    service.restore(&report.changed).unwrap();
    assert_eq!(service.count(None).unwrap(), 8);

    let cleared = service.clear_all(Some(ItemKind::Show)).unwrap();
    assert!(cleared.changed.is_empty());
    let cleared = service.clear_all(None).unwrap();
    assert_eq!(cleared.changed.len(), 8);
    assert_eq!(service.count(None).unwrap(), 0);
}

#[test]
fn clearing_new_everywhere_counts_titles_not_episodes() {
    let lab = Lab::new();
    let movies: Vec<ItemId> = (1..=3).map(|n| lab.movie(n, &[]).0).collect();
    let (show, _, episodes) = lab.show();
    let mut flagged = movies.clone();
    flagged.push(show);
    flagged.extend(episodes.iter().copied());
    lab.db.write(move |tx| set_new(tx, &flagged, true)).unwrap();

    let cleared = lab.flags().clear_all(None).unwrap();
    // Three movies, the show and its five episodes changed; that is four titles.
    assert_eq!(cleared.changed.len(), 9);
    assert_eq!(cleared.titles, 4);
}

/// reg2-3: a show carries the NEW marker in the grid and the table while one of its episodes is
/// NEW, as Home and the show page count it. Opening the show keeps its episodes NEW (they were not
/// opened); clearing NEW on the show clears its episodes too, and Undo brings both back.
#[test]
fn a_show_is_new_while_an_episode_is_and_clearing_it_clears_its_episodes() {
    let lab = Lab::new();
    let (show, _, episodes) = lab.show();
    let mut flagged = vec![show];
    flagged.extend(episodes.iter().copied());
    lab.db.write(move |tx| set_new(tx, &flagged, true)).unwrap();
    let marked = |id: ItemId| {
        lab.db
            .read(|c| cv_db::repo::get_summary(c, id))
            .unwrap()
            .unwrap()
            .is_new
    };
    let service = lab.flags();

    let opened = service.opened(show).unwrap();
    assert_eq!(opened.changed, vec![show], "only the show's own flag");
    assert!(marked(show), "its episodes are still new");

    let cleared = service.clear(&[show]).unwrap();
    assert_eq!(cleared.changed, episodes, "the episodes lose the flag too");
    assert_eq!(cleared.titles, 1);
    assert!(!marked(show));

    service.restore(&cleared.changed).unwrap();
    assert!(marked(show));
    let all_shows = service.clear_all(Some(ItemKind::Show)).unwrap();
    assert_eq!(all_shows.changed, episodes);
    assert!(!marked(show));
}
