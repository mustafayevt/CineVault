//! Incremental rescans of TV data sources: shows, seasons and episodes.

mod support;

use cv_db::repo::{FacetCondition, Filter, FilterId, ItemQuery, count_items};
use cv_domain::{EpisodeNumber, Item, ItemDetail, ItemKind, MediaFileType};
use support::Lib;

fn numbers(item: &Item) -> Vec<(u32, u32)> {
    let ItemDetail::Episode(e) = &item.detail else {
        panic!("an episode");
    };
    e.numbers
        .iter()
        .filter(|n| n.group == EpisodeNumber::aired(0, 0).group)
        .map(|n| (n.season, n.episode))
        .collect()
}

fn show_lib() -> Lib {
    let lib = Lib::tv();
    lib.put("Severance (2022)/tvshow.nfo", "<tvshow/>");
    lib.put("Severance (2022)/season01-poster.jpg", "p");
    lib.put("Severance (2022)/Season 01/Severance.S01E01.mkv", "e1");
    lib.put("Severance (2022)/Season 01/Severance.S01E02.mkv", "e2");
    lib.put("Severance (2022)/Season 02/Severance.S02E01.mkv", "e3");
    lib
}

/// reg3-1: an episode whose file name carries no title is stored without one, not with the
/// file name; a file name with a title gives that title.
#[test]
fn episodes_without_a_title_in_the_name_are_stored_without_one() {
    let lib = Lib::tv();
    lib.put("One Piece/[SubGroup] One Piece - 001 [1080p].mkv", "a");
    lib.put("One Piece/[SubGroup] One Piece - 002 [1080p].mkv", "b");
    lib.put("One Piece/[SubGroup] One Piece - 1001 [1080p].mkv", "c");
    lib.put("Dark/Dark - S01E02 - Lies.mkv", "d");
    lib.rescan();

    let mut titles: Vec<(Vec<(u32, u32)>, String)> = lib
        .items(ItemKind::Episode)
        .iter()
        .map(|e| (numbers(e), e.core.title.clone()))
        .collect();
    titles.sort();
    assert_eq!(
        titles,
        [
            (vec![(1, 1)], String::new()),
            (vec![(1, 2)], String::new()),
            (vec![(1, 2)], "Lies".to_owned()),
            (vec![(1, 1001)], String::new()),
        ]
    );
}

#[test]
fn first_scan_builds_show_seasons_and_episodes() {
    let lib = show_lib();
    let report = lib.rescan();

    assert_eq!(lib.items(ItemKind::Show).len(), 1);
    assert_eq!(lib.items(ItemKind::Season).len(), 2);
    let episodes = lib.items(ItemKind::Episode);
    assert_eq!(episodes.len(), 3);
    let show = &lib.items(ItemKind::Show)[0];
    assert_eq!(show.core.title, "Severance");
    assert_eq!(show.core.year, 2022);
    for episode in &episodes {
        assert_eq!(episode.parent_id, show.id);
    }
    let mut all: Vec<_> = episodes.iter().flat_map(numbers).collect();
    all.sort_unstable();
    assert_eq!(all, [(1, 1), (1, 2), (2, 1)]);

    // The show and its episodes are NEW; seasons are structure, not content.
    assert_eq!(report.new_items.len(), 4);
    assert_eq!(report.needs_media_info.len(), 3);
    // The season poster belongs to season 1.
    let season_one = lib
        .items(ItemKind::Season)
        .into_iter()
        .find(|s| matches!(&s.detail, ItemDetail::Season(f) if f.season == 1))
        .unwrap();
    assert!(
        season_one
            .core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::SeasonPoster
                || f.kind == MediaFileType::Poster
                || f.filename == "season01-poster.jpg")
    );
}

#[test]
fn unchanged_tv_rescan_modifies_nothing() {
    let lib = show_lib();
    lib.rescan();
    lib.read_media_info();
    let before = lib.content_hash();

    let report = lib.rescan();

    assert!(report.is_unchanged(), "{report:?}");
    assert!(report.needs_media_info.is_empty());
    assert_eq!(lib.content_hash(), before);
}

#[test]
fn a_new_episode_adds_only_that_episode() {
    let lib = show_lib();
    lib.rescan();
    let before = lib.item_count();

    lib.put("Severance (2022)/Season 02/Severance.S02E02.mkv", "e4");
    let report = lib.rescan();

    assert_eq!(report.new_items.len(), 1);
    assert_eq!(lib.item_count(), before + 1);
    assert_eq!(lib.items(ItemKind::Season).len(), 2);
}

#[test]
fn an_episode_in_a_new_season_creates_the_season() {
    let lib = show_lib();
    lib.rescan();

    lib.put("Severance (2022)/Season 03/Severance.S03E01.mkv", "e5");
    lib.rescan();

    assert_eq!(lib.items(ItemKind::Season).len(), 3);
    assert_eq!(lib.items(ItemKind::Episode).len(), 4);
}

#[test]
fn an_upgraded_episode_file_keeps_its_episode() {
    let lib = show_lib();
    lib.rescan();
    lib.read_media_info();
    let target = lib
        .items(ItemKind::Episode)
        .into_iter()
        .find(|e| numbers(e) == [(1, 2)])
        .unwrap();
    let id = target.id.unwrap();

    lib.remove("Severance (2022)/Season 01/Severance.S01E02.mkv");
    lib.put(
        "Severance (2022)/Season 01/Severance.S01E02.2160p.WEB-DL.mkv",
        "a larger upgrade of episode two",
    );
    let report = lib.rescan();

    assert!(report.new_items.is_empty(), "{report:?}");
    assert_eq!(report.updated, vec![id]);
    assert_eq!(report.stats.replaced, 1);
    let upgraded = lib.item(id);
    assert!(
        upgraded
            .core
            .media_files
            .iter()
            .any(|f| f.filename == "Severance.S01E02.2160p.WEB-DL.mkv" && !f.has_media_info())
    );
    assert_eq!(lib.items(ItemKind::Episode).len(), 3);
}

#[test]
fn a_multi_episode_file_makes_one_item_per_number_sharing_the_file() {
    let lib = Lib::tv();
    lib.put("Lost (2004)/Season 02/Lost.S02E05-E07.mkv", "triple");
    lib.rescan();

    let episodes = lib.items(ItemKind::Episode);
    assert_eq!(episodes.len(), 3);
    for episode in &episodes {
        assert_eq!(episode.core.media_files.len(), 1);
    }
    let mut all: Vec<_> = episodes.iter().flat_map(numbers).collect();
    all.sort_unstable();
    assert_eq!(all, [(2, 5), (2, 6), (2, 7)]);

    lib.read_media_info();
    let before = lib.content_hash();
    assert!(lib.rescan().is_unchanged());
    assert_eq!(lib.content_hash(), before);

    // Replacing the shared file updates all three.
    lib.put(
        "Lost (2004)/Season 02/Lost.S02E05-E07.mkv",
        "triple, remuxed",
    );
    let report = lib.rescan();
    assert_eq!(report.updated.len(), 3);
}

#[test]
fn a_deleted_show_folder_makes_its_episodes_missing_but_keeps_everything() {
    let lib = show_lib();
    lib.rescan();
    let count = lib.item_count();

    lib.remove("Severance (2022)");
    let report = lib.rescan();

    assert_eq!(lib.item_count(), count);
    // Nothing is found under the source any more, so the walk is empty and not trusted; the
    // folder itself is gone, so the episodes are held in their previous state.
    assert!(report.availability.became_missing.is_empty());
    assert_eq!(lib.items(ItemKind::Episode).len(), 3);
}

#[test]
fn a_deleted_show_folder_with_other_shows_left_makes_its_episodes_missing() {
    let lib = show_lib();
    lib.put("Andor (2022)/Season 01/Andor.S01E01.mkv", "a");
    lib.rescan();

    lib.remove("Severance (2022)");
    let report = lib.rescan();

    // The show and its three episodes.
    assert_eq!(report.availability.became_missing.len(), 4);
    for episode in lib.items(ItemKind::Episode) {
        let missing = lib.availability(episode.id.unwrap()) == "missing";
        assert_eq!(missing, episode.core.path.contains("Severance"));
    }
}

// ---- new-episode tracking and duplicate episodes ---------------------------------

fn episode_at(lib: &support::Lib, season: u32, number: u32) -> Vec<Item> {
    lib.items(ItemKind::Episode)
        .into_iter()
        .filter(|e| numbers(e) == [(season, number)])
        .collect()
}

fn new_children(lib: &support::Lib, show: cv_domain::ItemId) -> u64 {
    lib.db
        .read(|conn| cv_db::repo::count_new_children(conn, show))
        .unwrap()
}

#[test]
fn a_weekly_episode_is_flagged_new_and_counted_on_its_show() {
    let lib = show_lib();
    lib.rescan();
    let show = lib.items(ItemKind::Show)[0].id.unwrap();

    // The user has seen what the first scan found.
    lib.db
        .write(move |conn| cv_db::repo::clear_new_children(conn, show))
        .unwrap();
    assert_eq!(new_children(&lib, show), 0);

    lib.put("Severance (2022)/Season 02/Severance.S02E02.mkv", "weekly");
    let report = lib.rescan();

    let added = episode_at(&lib, 2, 2);
    assert_eq!(added.len(), 1);
    let id = added[0].id.unwrap();
    assert_eq!(report.new_items, vec![id]);
    assert!(lib.is_new(id));
    assert_eq!(new_children(&lib, show), 1, "the show says 1 new episode");
    for old in episode_at(&lib, 2, 1) {
        assert!(!lib.is_new(old.id.unwrap()));
    }
}

#[test]
fn a_repack_beside_the_original_makes_two_episodes_for_one_number() {
    let lib = Lib::tv();
    lib.put("Severance (2022)/Season 01/S01E03.mkv", "original");
    lib.put("Severance (2022)/Season 01/S01E04.mkv", "single");
    lib.put("Severance (2022)/Season 01/S01E03.REPACK.mkv", "repack");
    lib.rescan();

    // Both files are kept and both claim S01E03.
    assert_eq!(episode_at(&lib, 1, 3).len(), 2);
    assert_eq!(episode_at(&lib, 1, 4).len(), 1);

    let flagged = |filter: Filter| {
        lib.db
            .read(|conn| count_items(conn, &ItemQuery::new(ItemKind::Show).filter(filter)))
            .unwrap()
    };
    let duplicates = Filter::Facet(FacetCondition::new(FilterId::DuplicateEpisodes));
    assert_eq!(
        flagged(duplicates.clone()),
        1,
        "the show has duplicate files"
    );

    // Once one of the two files is gone the clash is over, although the row is kept.
    let repack = episode_at(&lib, 1, 3)[1].id.unwrap();
    lib.db
        .write(move |conn| {
            cv_db::repo::set_availability(
                conn,
                repack,
                cv_domain::Availability::Missing,
                "the file is gone",
            )
        })
        .unwrap();
    assert_eq!(
        flagged(duplicates.clone()),
        0,
        "a deleted file is not a duplicate"
    );
    lib.db
        .write(move |conn| {
            cv_db::repo::set_availability(conn, repack, cv_domain::Availability::Online, "")
        })
        .unwrap();
    assert_eq!(flagged(duplicates), 1);

    // "Continuing shows with new episodes" is the status filter and the new-episode filter. A
    // scanned show has no status until it is scraped.
    let show = lib.items(ItemKind::Show)[0].id.unwrap();
    lib.db
        .write(move |conn| {
            let mut item = cv_db::repo::get_item(conn, show)?.expect("show");
            if let ItemDetail::Show(fields) = &mut item.detail {
                fields.status = cv_domain::AiredStatus::Continuing;
            }
            cv_db::repo::update_item(conn, &item)
        })
        .unwrap();
    let status = Filter::Facet(FacetCondition::new(FilterId::Status).with_values(["continuing"]));
    let new_episodes = Filter::Facet(FacetCondition::new(FilterId::NewEpisodes));
    let both = Filter::and([status, new_episodes]);
    assert_eq!(flagged(both), 1);
}

#[test]
fn theme_music_in_the_show_folder_is_listed_on_the_show_also_when_added_later() {
    let lib = show_lib();
    lib.rescan();
    lib.put("Severance (2022)/theme.mp3", "tune");
    lib.rescan();

    let show = &lib.items(ItemKind::Show)[0];
    assert!(
        show.core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::Theme && f.filename == "theme.mp3"),
        "{:?}",
        show.core.media_files
    );
}

#[test]
fn a_show_that_is_now_a_season_folder_of_another_show_is_marked_missing_not_kept_online() {
    use cv_db::repo::insert_item;

    // The library folder is one show: it holds a season folder.
    let lib = Lib::tv();
    lib.put("Season 1/Chernobyl S01E01.mkv", "e1");
    lib.put("Season 1/Chernobyl S01E02.mkv", "e2");
    // An earlier reading of the tree took `Season 1` for a show of its own, with the same files.
    let season_dir = lib.root.join("Season 1").to_string_lossy().into_owned();
    let root = lib.root.to_string_lossy().into_owned();
    let stale = {
        let (season_dir, root) = (season_dir.clone(), root.clone());
        lib.db
            .write(move |tx| {
                let mut show = Item::show("Season 1");
                show.core.path = season_dir.clone();
                show.core.data_source = root.clone();
                let show = insert_item(tx, &show)?;
                for (number, name) in [(1, "Chernobyl S01E01.mkv"), (2, "Chernobyl S01E02.mkv")] {
                    let mut episode = Item::episode(show, name);
                    episode.core.path = season_dir.clone();
                    episode.core.data_source = root.clone();
                    episode.core.media_files.push(cv_domain::MediaFile::new(
                        MediaFileType::Video,
                        season_dir.clone(),
                        name,
                    ));
                    if let ItemDetail::Episode(e) = &mut episode.detail {
                        e.numbers.set(EpisodeNumber::aired(1, number));
                    }
                    insert_item(tx, &episode)?;
                }
                Ok(show)
            })
            .unwrap()
    };

    let report = lib.rescan();
    let shows = lib.items(ItemKind::Show);
    assert_eq!(shows.len(), 2, "nothing is deleted");
    let current = shows.iter().find(|s| s.id != Some(stale)).unwrap();
    assert_eq!(lib.availability(current.id.unwrap()), "online");
    assert_eq!(
        lib.availability(stale),
        "missing",
        "the leftover show is flagged"
    );
    for episode in lib.items(ItemKind::Episode) {
        let expected = if episode.parent_id == Some(stale) {
            "missing"
        } else {
            "online"
        };
        assert_eq!(
            lib.availability(episode.id.unwrap()),
            expected,
            "{}",
            episode.core.title
        );
    }
    assert!(report.availability.became_missing.contains(&stale));
    let reason: String = lib
        .db
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT availability_reason FROM item WHERE id = ?1",
                [stale.0],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(reason.contains("part of the show"), "{reason}");

    // A second rescan keeps the verdict instead of putting the show back online.
    let again = lib.rescan();
    assert_eq!(lib.availability(stale), "missing");
    assert!(
        again.availability.restored.is_empty(),
        "{:?}",
        again.availability
    );
}

/// reg2-1: two stored shows on one folder (a rescan that once raced an organize added the show a
/// second time). The rescan keeps the oldest online and flags the other as missing with the reason,
/// so it leaves the Organize conflict list and can be removed; nothing is deleted.
#[test]
fn a_second_show_on_the_same_folder_is_marked_missing_and_the_oldest_kept() {
    use cv_db::repo::insert_item;

    let lib = Lib::tv();
    lib.put("The Office (US)/Season 1/The Office (US) S01E01.mkv", "e1");
    lib.rescan();
    let kept = lib.item_ids(ItemKind::Show)[0];
    let folder = lib
        .root
        .join("The Office (US)")
        .to_string_lossy()
        .into_owned();
    let season_dir = lib
        .root
        .join("The Office (US)/Season 1")
        .to_string_lossy()
        .into_owned();
    let root = lib.root.to_string_lossy().into_owned();
    let (double, double_episode) = lib
        .db
        .write(move |tx| {
            let mut show = Item::show("The Office US");
            show.core.path = folder.clone();
            show.core.data_source = root.clone();
            let show = insert_item(tx, &show)?;
            let mut episode = Item::episode(show, "S01E01");
            episode.core.path = season_dir.clone();
            episode.core.data_source = root.clone();
            episode.core.media_files.push(cv_domain::MediaFile::new(
                MediaFileType::Video,
                season_dir.clone(),
                "The Office (US) S01E01.mkv",
            ));
            if let ItemDetail::Episode(e) = &mut episode.detail {
                e.numbers.set(EpisodeNumber::aired(1, 1));
            }
            let episode = insert_item(tx, &episode)?;
            Ok((show, episode))
        })
        .unwrap();

    let report = lib.rescan();
    assert_eq!(lib.items(ItemKind::Show).len(), 2, "nothing is deleted");
    assert_eq!(lib.availability(kept), "online");
    assert_eq!(lib.availability(double), "missing");
    assert_eq!(lib.availability(double_episode), "missing");
    assert!(report.availability.became_missing.contains(&double));
    let reason: String = lib
        .db
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT availability_reason FROM item WHERE id = ?1",
                [double.0],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(reason.contains("belongs to another show"), "{reason}");
    assert_eq!(
        lib.items(ItemKind::Show).len(),
        2,
        "a second rescan adds nothing"
    );
    lib.rescan();
    assert_eq!(lib.availability(double), "missing");
    assert_eq!(lib.availability(kept), "online");
}

#[test]
fn a_removed_tv_folder_added_back_as_its_show_folder_finds_the_show_again() {
    use cv_db::repo::data_sources::{insert_source, remove_source};
    use cv_domain::SourceKind;

    // The library folder "Mixed" holds one show.
    let lib = Lib::tv();
    lib.put("Chernobyl/Season 1/Chernobyl S01E01.mkv", "e1");
    lib.put("Chernobyl/Season 1/Chernobyl S01E02.mkv", "e2");
    lib.rescan();
    let show = lib.items(ItemKind::Show)[0].id.unwrap();
    let episodes = lib.item_ids(ItemKind::Episode);

    // The folder is removed (its titles stay), then its show folder is added as a TV folder
    // and the old folder as a movie folder.
    let id = lib.source.id;
    lib.db.write(move |tx| remove_source(tx, id)).unwrap();
    let show_root = lib.root.join("Chernobyl").to_string_lossy().into_owned();
    let tv = lib
        .db
        .write(move |tx| insert_source(tx, SourceKind::Tv, &show_root))
        .unwrap();
    let movie_root = lib.root.to_string_lossy().into_owned();
    let movies = lib
        .db
        .write(move |tx| insert_source(tx, SourceKind::Movie, &movie_root))
        .unwrap();
    let options = lib.options();
    let tv_report = cv_scan::rescan::rescan_source(&lib.db, &tv, &options).unwrap();
    cv_scan::rescan::rescan_source(&lib.db, &movies, &options).unwrap();
    cv_scan::rescan::refresh_detached(&lib.db).unwrap();

    // One show: the old row now belongs to the new folder, with its episodes, online.
    assert!(tv_report.new_items.is_empty(), "{:?}", tv_report.new_items);
    assert_eq!(lib.item_ids(ItemKind::Show), [show]);
    assert_eq!(lib.item_ids(ItemKind::Episode), episodes);
    let stored = lib.item(show);
    assert_eq!(stored.core.data_source, tv.path);
    assert_eq!(lib.availability(show), "online");
    for episode in episodes {
        assert_eq!(lib.item(episode).core.data_source, tv.path);
        assert_eq!(lib.availability(episode), "online");
    }
}

#[test]
fn a_show_and_its_episodes_get_an_added_date_on_the_first_scan() {
    let lib = show_lib();
    lib.rescan();

    for kind in [ItemKind::Show, ItemKind::Episode] {
        let items = lib.items(kind);
        assert!(!items.is_empty());
        assert!(
            items
                .iter()
                .all(|item| item.core.date_added_ms == 1_700_000_000_000),
            "{kind:?}"
        );
    }
}
