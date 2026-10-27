use cv_domain::{Item, MediaFileType};

use super::*;
use crate::error::RenameError;
use crate::movie::{MovieInput, MovieRenamer};
use crate::options::MovieSettings;
use crate::plan::{Owner, PlanFile};

const ROOT: &str = "D:/Movies";

fn opts(case_insensitive: bool) -> ConflictOptions {
    ConflictOptions {
        case_insensitive,
        limits: PathLimits::windows_long_paths(),
        allow_merge: false,
        name_options: NameOptions::default(),
        space: SpaceSubstitution::default(),
    }
}

fn carried(from: &str, kind: MediaFileType, to: &[&str]) -> PlannedFile {
    PlannedFile {
        from: from.to_owned(),
        kind,
        owner: Owner::Movie,
        fate: Fate::Carried {
            to: to.iter().map(|t| (*t).to_owned()).collect(),
        },
    }
}

fn item(
    id: &str,
    old: &str,
    new: &str,
    change: FolderChange,
    files: Vec<PlannedFile>,
) -> BatchItem {
    BatchItem {
        id: id.to_owned(),
        root: ROOT.to_owned(),
        old_folder: old.to_owned(),
        new_folder: new.to_owned(),
        folder_change: change,
        base_name: None,
        version_label: None,
        shares_folder: false,
        files,
    }
}

fn kinds(report: &ConflictReport) -> Vec<&ConflictKind> {
    report.conflicts.iter().map(|c| &c.kind).collect()
}

/// A movie planned by the real planner: files `<folder>/movie.mkv` and friends.
fn planned_movie(id: &str, folder: &str, title: &str, year: u16) -> BatchItem {
    let mut movie = Item::movie(title);
    movie.core.year = year;
    let files = vec![
        PlanFile::new(format!("{folder}/movie.mkv"), MediaFileType::Video),
        PlanFile::new(format!("{folder}/movie.nfo"), MediaFileType::Nfo),
        PlanFile::new(format!("{folder}/movie-poster.jpg"), MediaFileType::Poster),
        PlanFile::new(format!("{folder}/fanart.jpg"), MediaFileType::Fanart),
    ];
    let plan = MovieRenamer::new(MovieSettings::default())
        .plan(&MovieInput {
            item: &movie,
            set: None,
            folder,
            files: &files,
        })
        .expect("plans");
    BatchItem::from_movie(id, ROOT, &plan)
}

fn targets(item: &BatchItem, from_suffix: &str) -> Vec<String> {
    let file = item
        .files
        .iter()
        .find(|f| f.from.ends_with(from_suffix))
        .unwrap_or_else(|| panic!("no file {from_suffix}"));
    match &file.fate {
        Fate::Carried { to } => to.clone(),
        other => panic!("{other:?}"),
    }
}

fn two_versions() -> Vec<BatchItem> {
    vec![
        planned_movie("a", "dl-1", "Les Misérables", 2012),
        planned_movie("b", "dl-2", "Les Misérables", 2012),
    ]
}

#[test]
fn a_clean_batch_has_no_conflicts() {
    let items = vec![
        planned_movie("a", "dl-1", "Heat", 1995),
        planned_movie("b", "dl-2", "Alien", 1979),
    ];
    let report = detect(&items, &MemoryDisk::new(true), &opts(true));
    assert!(report.is_empty(), "{report:?}");
    assert_eq!(applicable(&items, &report).len(), 2);
}

// Two rows of one show, one under the old library folder and one under its show folder added
// as a library folder of its own, plan the same files onto the same destination.
#[test]
fn items_under_nested_roots_that_claim_one_file_collide() {
    let mut outer = item(
        "28",
        "Chernobyl",
        "Chernobyl",
        FolderChange::Unchanged,
        vec![carried(
            "Chernobyl/Season 1/Chernobyl S01E01.mkv",
            MediaFileType::Video,
            &["Chernobyl/Season 01/Chernobyl S01E01.mkv"],
        )],
    );
    outer.root = "D:/TV/Mixed".to_owned();
    let mut inner = item(
        "73",
        "",
        "",
        FolderChange::Unchanged,
        vec![carried(
            "Season 1/Chernobyl S01E01.mkv",
            MediaFileType::Video,
            &["Season 01/Chernobyl S01E01.mkv"],
        )],
    );
    inner.root = "D:\\TV\\Mixed\\Chernobyl\\".to_owned();
    let items = vec![outer, inner];
    let report = detect(&items, &MemoryDisk::new(true), &opts(true));
    assert!(
        report
            .conflicts
            .iter()
            .any(|c| c.kind == ConflictKind::DuplicateDestination
                && c.items == ["28".to_owned(), "73".to_owned()]),
        "{report:?}"
    );
    assert!(applicable(&items, &report).is_empty());
}

// Spec scenario: two folders would both become `Les Misérables (2012)`.
#[test]
fn two_versions_collide_and_are_blocked() {
    let items = two_versions();
    assert_eq!(items[0].new_folder, "Les Misérables (2012)");
    assert_eq!(items[1].new_folder, "Les Misérables (2012)");
    let report = detect(&items, &MemoryDisk::new(true), &opts(true));
    assert!(!report.is_empty());
    assert!(
        report
            .conflicts
            .iter()
            .all(|c| c.kind == ConflictKind::DuplicateDestination)
    );
    // Both items are in every conflict, in batch order.
    assert!(
        report
            .conflicts
            .iter()
            .all(|c| c.items == ["a".to_owned(), "b".to_owned()])
    );
    assert_eq!(report.blocked_items().len(), 2);
    assert!(applicable(&items, &report).is_empty());
    let video = report
        .conflicts
        .iter()
        .find(|c| c.path.ends_with(".mkv"))
        .expect("video conflict");
    assert_eq!(
        video.resolutions(),
        [
            ResolutionKind::KeepAsVersions,
            ResolutionKind::Skip,
            ResolutionKind::Edit
        ]
    );
}

#[test]
fn keep_both_as_versions_clears_the_conflict() {
    let items = vec![
        planned_movie("a", "dl-1", "Les Misérables", 2012).with_version_label("1080p"),
        planned_movie("b", "dl-2", "Les Misérables", 2012).with_version_label("720p"),
    ];
    let choices = BTreeMap::from([
        ("a".to_owned(), Resolution::KeepAsVersions),
        ("b".to_owned(), Resolution::KeepAsVersions),
    ]);
    let disk = MemoryDisk::new(true);
    let resolved = resolve(&items, &choices, &disk, &opts(true)).expect("resolves");
    assert!(resolved.skipped.is_empty());
    assert!(
        detect(&resolved.items, &disk, &opts(true)).is_empty(),
        "{:?}",
        detect(&resolved.items, &disk, &opts(true))
    );
    let (a, b) = (&resolved.items[0], &resolved.items[1]);
    assert_eq!(
        targets(a, "movie.mkv"),
        ["Les Misérables (2012)/Les Misérables (2012) - 1080p.mkv"]
    );
    assert_eq!(
        targets(b, "movie.mkv"),
        ["Les Misérables (2012)/Les Misérables (2012) - 720p.mkv"]
    );
    assert_eq!(
        targets(b, "movie.nfo"),
        ["Les Misérables (2012)/Les Misérables (2012) - 720p.nfo"]
    );
    assert_eq!(
        targets(a, "fanart.jpg"),
        ["Les Misérables (2012)/Les Misérables (2012) - 1080p-fanart.jpg"]
    );
}

#[test]
fn fixed_names_exist_once_among_versions() {
    let twin = |id: &str, old: &str| {
        let mut item = item(
            id,
            old,
            "Heat (1995)",
            FolderChange::Rename,
            vec![
                carried(
                    &format!("{old}/m.mkv"),
                    MediaFileType::Video,
                    &["Heat (1995)/Heat (1995).mkv"],
                ),
                carried(
                    &format!("{old}/poster.jpg"),
                    MediaFileType::Poster,
                    &["Heat (1995)/poster.jpg"],
                ),
            ],
        );
        item.base_name = Some("Heat (1995)".to_owned());
        item
    };
    let items = [twin("a", "dl-1"), twin("b", "dl-2")];
    let choices = BTreeMap::from([
        ("a".to_owned(), Resolution::KeepAsVersions),
        ("b".to_owned(), Resolution::KeepAsVersions),
    ]);
    let disk = MemoryDisk::new(true);
    let resolved = resolve(&items, &choices, &disk, &opts(true)).unwrap();
    assert!(detect(&resolved.items, &disk, &opts(true)).is_empty());
    let (a, b) = (&resolved.items[0], &resolved.items[1]);
    assert_eq!(targets(a, "poster.jpg"), ["Heat (1995)/poster.jpg"]);
    let poster = b
        .files
        .iter()
        .find(|f| f.from.ends_with("poster.jpg"))
        .unwrap();
    assert_eq!(poster.fate, Fate::Trash);
    assert!(
        b.ops()
            .iter()
            .any(|op| op.kind == crate::plan::OpKind::Trash)
    );
    // The videos are never dropped.
    assert_eq!(
        targets(b, "m.mkv"),
        ["Heat (1995)/Heat (1995) - Version 2.mkv"]
    );
}

#[test]
fn versions_without_labels_are_numbered() {
    let items = two_versions();
    let choices = BTreeMap::from([
        ("a".to_owned(), Resolution::KeepAsVersions),
        ("b".to_owned(), Resolution::KeepAsVersions),
    ]);
    let disk = MemoryDisk::new(false);
    let resolved = resolve(&items, &choices, &disk, &opts(false)).unwrap();
    assert_eq!(
        targets(&resolved.items[0], "movie.mkv"),
        ["Les Misérables (2012)/Les Misérables (2012) - Version 1.mkv"]
    );
    assert_eq!(
        targets(&resolved.items[1], "movie.mkv"),
        ["Les Misérables (2012)/Les Misérables (2012) - Version 2.mkv"]
    );
    assert!(detect(&resolved.items, &disk, &opts(false)).is_empty());
}

#[test]
fn equal_labels_are_made_unique() {
    let items = vec![
        planned_movie("a", "dl-1", "Heat", 1995).with_version_label("1080p"),
        planned_movie("b", "dl-2", "Heat", 1995).with_version_label("1080P"),
    ];
    let choices = BTreeMap::from([
        ("a".to_owned(), Resolution::KeepAsVersions),
        ("b".to_owned(), Resolution::KeepAsVersions),
    ]);
    let disk = MemoryDisk::new(true);
    let resolved = resolve(&items, &choices, &disk, &opts(true)).unwrap();
    assert!(detect(&resolved.items, &disk, &opts(true)).is_empty());
}

#[test]
fn one_side_can_be_versioned_next_to_an_unresolved_twin() {
    let items = two_versions();
    let choices = BTreeMap::from([("b".to_owned(), Resolution::KeepAsVersions)]);
    let disk = MemoryDisk::new(true);
    let resolved = resolve(&items, &choices, &disk, &opts(true)).unwrap();
    assert!(detect(&resolved.items, &disk, &opts(true)).is_empty());
    assert_eq!(
        targets(&resolved.items[0], "movie.mkv"),
        ["Les Misérables (2012)/Les Misérables (2012).mkv"]
    );
    assert_eq!(
        targets(&resolved.items[1], "movie.mkv"),
        ["Les Misérables (2012)/Les Misérables (2012) - Version 2.mkv"]
    );
}

#[test]
fn skip_removes_the_item_and_unblocks_its_twin() {
    let items = two_versions();
    let choices = BTreeMap::from([("b".to_owned(), Resolution::Skip)]);
    let disk = MemoryDisk::new(true);
    let resolved = resolve(&items, &choices, &disk, &opts(true)).unwrap();
    assert_eq!(resolved.skipped, ["b"]);
    assert_eq!(resolved.items.len(), 1);
    assert!(detect(&resolved.items, &disk, &opts(true)).is_empty());
}

#[test]
fn edit_the_folder_and_the_base_name() {
    let items = two_versions();
    let choices = BTreeMap::from([(
        "b".to_owned(),
        Resolution::Edit(NameEdit {
            folder: Some("Les Misérables (2012) Extended".to_owned()),
            base: Some("Les Misérables (2012) Extended".to_owned()),
        }),
    )]);
    let disk = MemoryDisk::new(true);
    let resolved = resolve(&items, &choices, &disk, &opts(true)).unwrap();
    assert!(detect(&resolved.items, &disk, &opts(true)).is_empty());
    let b = &resolved.items[1];
    assert_eq!(b.new_folder, "Les Misérables (2012) Extended");
    assert_eq!(
        targets(b, "movie.mkv"),
        ["Les Misérables (2012) Extended/Les Misérables (2012) Extended.mkv"]
    );
    assert_eq!(
        targets(b, "fanart.jpg"),
        ["Les Misérables (2012) Extended/Les Misérables (2012) Extended-fanart.jpg"]
    );
}

#[test]
fn an_edited_name_is_cleaned_and_must_not_be_empty() {
    let items = two_versions();
    let disk = MemoryDisk::new(true);
    let edit = |folder: &str| {
        BTreeMap::from([(
            "b".to_owned(),
            Resolution::Edit(NameEdit {
                folder: Some(folder.to_owned()),
                base: None,
            }),
        )])
    };
    let resolved = resolve(&items, &edit("Les Mis: Extended?"), &disk, &opts(true)).unwrap();
    assert_eq!(resolved.items[1].new_folder, "Les Mis - Extended");
    let err = resolve(&items, &edit(" ../ "), &disk, &opts(true)).unwrap_err();
    assert!(matches!(err, RenameError::EmptyEditedName { item } if item == "b"));
}

#[test]
fn a_base_name_cannot_be_edited_on_a_show() {
    let show = item(
        "s",
        "old",
        "New",
        FolderChange::Rename,
        vec![carried("old/a.mkv", MediaFileType::Video, &["New/a.mkv"])],
    );
    let choices = BTreeMap::from([(
        "s".to_owned(),
        Resolution::Edit(NameEdit {
            folder: None,
            base: Some("x".to_owned()),
        }),
    )]);
    let err = resolve(
        std::slice::from_ref(&show),
        &choices,
        &MemoryDisk::new(false),
        &opts(false),
    )
    .unwrap_err();
    assert!(matches!(err, RenameError::ResolutionNotApplicable { .. }));
    let versions = BTreeMap::from([("s".to_owned(), Resolution::KeepAsVersions)]);
    // Nothing collides, so nothing needs a label.
    assert!(resolve(&[show], &versions, &MemoryDisk::new(false), &opts(false)).is_ok());
}

#[test]
fn a_folder_rename_onto_another_items_folder_is_foreign() {
    let items = vec![planned_movie("a", "dl-1", "Heat", 1995)];
    let disk = MemoryDisk::new(true)
        .with_dir(ROOT, "Heat (1995)", false, Some("other"))
        .with_file(ROOT, "Heat (1995)/Heat (1995).mkv", Some("other"));
    let report = detect(&items, &disk, &opts(true));
    assert_eq!(
        kinds(&report),
        [&ConflictKind::ForeignDestination {
            owner: Some("other".to_owned()),
            is_dir: true
        }]
    );
    assert_eq!(report.conflicts[0].path, "Heat (1995)");
    assert_eq!(report.conflicts[0].items, ["a"]);
    assert!(applicable(&items, &report).is_empty());
}

#[test]
fn merging_into_a_folder_needs_permission_and_an_empty_one_is_fine() {
    let items = vec![planned_movie("a", "dl-1", "Heat", 1995)];
    let full = MemoryDisk::new(true).with_dir(ROOT, "Heat (1995)", false, None);
    assert_eq!(detect(&items, &full, &opts(true)).conflicts.len(), 1);
    let mut allow = opts(true);
    allow.allow_merge = true;
    assert!(detect(&items, &full, &allow).is_empty());
    let empty = MemoryDisk::new(true).with_dir(ROOT, "Heat (1995)", true, None);
    assert!(detect(&items, &empty, &opts(true)).is_empty());
}

#[test]
fn an_existing_unknown_file_blocks_a_move_into_a_shared_folder() {
    let items = vec![item(
        "a",
        "",
        "M",
        FolderChange::MoveFiles,
        vec![carried("heat.mkv", MediaFileType::Video, &["M/Heat.mkv"])],
    )];
    let disk = MemoryDisk::new(false).with_file(ROOT, "M/Heat.mkv", None);
    let report = detect(&items, &disk, &opts(false));
    assert_eq!(
        kinds(&report),
        [&ConflictKind::ForeignDestination {
            owner: None,
            is_dir: false
        }]
    );
    // The same name in another case is another file on a case-sensitive volume.
    let other_case = MemoryDisk::new(false).with_file(ROOT, "M/heat.mkv", None);
    assert!(detect(&items, &other_case, &opts(false)).is_empty());
}

#[test]
fn the_items_own_files_and_vacated_paths_are_not_foreign() {
    // a replaces its own old nfo; b moves into the path a vacates. M is a shared folder.
    let mut a = item(
        "a",
        "",
        "M",
        FolderChange::MoveFiles,
        vec![
            carried("M/old.nfo", MediaFileType::Nfo, &["M/Heat.nfo"]),
            carried("M/Heat.mkv", MediaFileType::Video, &["M/Heat 1995.mkv"]),
        ],
    );
    let mut b = item(
        "b",
        "",
        "M",
        FolderChange::MoveFiles,
        vec![carried("other.mkv", MediaFileType::Video, &["M/Heat.mkv"])],
    );
    a.shares_folder = true;
    b.shares_folder = true;
    let disk = MemoryDisk::new(false)
        .with_file(ROOT, "M/Heat.nfo", Some("a"))
        .with_file(ROOT, "M/Heat.mkv", Some("a"))
        .with_file(ROOT, "M/old.nfo", Some("a"));
    assert!(detect(&[a, b], &disk, &opts(false)).is_empty());
}

#[test]
fn renaming_a_folder_onto_one_the_batch_vacates_is_not_foreign() {
    let a = item(
        "a",
        "A",
        "B",
        FolderChange::Rename,
        vec![carried("A/x.mkv", MediaFileType::Video, &["B/x.mkv"])],
    );
    let b = item(
        "b",
        "B",
        "C",
        FolderChange::Rename,
        vec![carried("B/y.mkv", MediaFileType::Video, &["C/y.mkv"])],
    );
    let disk = MemoryDisk::new(false)
        .with_dir(ROOT, "B", false, Some("b"))
        .with_file(ROOT, "B/y.mkv", Some("b"));
    assert!(detect(&[a, b], &disk, &opts(false)).is_empty());
}

#[test]
fn versioning_a_twin_of_an_existing_item_numbers_it_after_that_item() {
    let items = vec![item(
        "a",
        "",
        "M",
        FolderChange::MoveFiles,
        vec![carried(
            "heat.mkv",
            MediaFileType::Video,
            &["M/Heat (1995).mkv"],
        )],
    )];
    let mut items = items;
    items[0].base_name = Some("Heat (1995)".to_owned());
    let disk = MemoryDisk::new(false).with_file(ROOT, "M/Heat (1995).mkv", Some("other"));
    let choices = BTreeMap::from([("a".to_owned(), Resolution::KeepAsVersions)]);
    let resolved = resolve(&items, &choices, &disk, &opts(false)).unwrap();
    assert_eq!(
        targets(&resolved.items[0], "heat.mkv"),
        ["M/Heat (1995) - Version 2.mkv"]
    );
    assert!(detect(&resolved.items, &disk, &opts(false)).is_empty());
}

#[test]
fn a_path_over_the_limit_is_a_conflict_with_the_root_counted() {
    let name = "x".repeat(100);
    let long = format!("{name} (2000)/{name} (2000).mkv");
    let items = vec![item(
        "a",
        "old",
        &format!("{name} (2000)"),
        FolderChange::Rename,
        vec![carried("old/a.mkv", MediaFileType::Video, &[long.as_str()])],
    )];
    let mut windows = opts(true);
    windows.limits = PathLimits::windows();
    let disk = MemoryDisk::new(true);
    // 9 + 1 + 219 = 229: fine; with a 43 character root it is 263.
    assert!(detect(&items, &disk, &windows).is_empty());
    let mut deep = items.clone();
    deep[0].root = format!("D:/{}", "r".repeat(40));
    let report = detect(&deep, &disk, &windows);
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(
        report.conflicts[0].kind,
        ConflictKind::PathTooLong {
            length: 263,
            limit: 259
        }
    );
    assert_eq!(report.conflicts[0].path, long);
    assert_eq!(
        report.conflicts[0].resolutions(),
        [ResolutionKind::Skip, ResolutionKind::Edit]
    );
    // Long paths lift the limit.
    let mut long_paths = opts(true);
    long_paths.limits = PathLimits::windows_long_paths();
    assert!(detect(&deep, &disk, &long_paths).is_empty());
}

#[test]
fn a_name_over_the_limit_is_reported_once_per_item_with_the_longest() {
    let n300 = "n".repeat(300);
    let n260 = "m".repeat(260);
    let items = vec![item(
        "a",
        "old",
        &n300,
        FolderChange::Rename,
        vec![
            carried(
                "old/a.mkv",
                MediaFileType::Video,
                &[format!("{n300}/a.mkv").as_str()],
            ),
            carried(
                "old/b.mkv",
                MediaFileType::Video,
                &[format!("{n300}/{n260}.mkv").as_str()],
            ),
        ],
    )];
    let report = detect(&items, &MemoryDisk::new(false), &opts(false));
    let names: Vec<_> = report
        .conflicts
        .iter()
        .filter(|c| matches!(c.kind, ConflictKind::NameTooLong { .. }))
        .collect();
    assert_eq!(names.len(), 1);
    assert_eq!(
        names[0].kind,
        ConflictKind::NameTooLong {
            length: 300,
            limit: 255
        }
    );
}

#[test]
fn lengths_are_counted_in_the_unit_of_the_file_system() {
    // 100 characters of 3 bytes each: 100 UTF-16 units, 300 UTF-8 bytes.
    let name = "界".repeat(100);
    let items = vec![item(
        "a",
        "",
        "M",
        FolderChange::MoveFiles,
        vec![carried(
            "a.mkv",
            MediaFileType::Video,
            &[format!("M/{name}.mkv").as_str()],
        )],
    )];
    let disk = MemoryDisk::new(false);
    let mut utf16 = opts(false);
    utf16.limits = PathLimits::windows();
    assert!(detect(&items, &disk, &utf16).is_empty());
    let mut bytes = opts(false);
    bytes.limits = PathLimits::macos();
    let report = detect(&items, &disk, &bytes);
    assert_eq!(
        kinds(&report),
        [&ConflictKind::NameTooLong {
            length: 304,
            limit: 255
        }]
    );
}

#[test]
fn a_case_only_move_is_safe_but_a_case_only_copy_is_not() {
    let moved = item(
        "a",
        "M",
        "M",
        FolderChange::Unchanged,
        vec![carried("M/heat.mkv", MediaFileType::Video, &["M/Heat.mkv"])],
    );
    let disk = MemoryDisk::new(true).with_file(ROOT, "M/heat.mkv", Some("a"));
    assert!(detect(&[moved], &disk, &opts(true)).is_empty());

    // The poster is copied to a name that is the source in another case.
    let copied = item(
        "a",
        "M",
        "M",
        FolderChange::Unchanged,
        vec![carried(
            "M/poster.jpg",
            MediaFileType::Poster,
            &["M/Heat-poster.jpg", "M/Poster.jpg"],
        )],
    );
    let disk = MemoryDisk::new(true).with_file(ROOT, "M/poster.jpg", Some("a"));
    let report = detect(std::slice::from_ref(&copied), &disk, &opts(true));
    assert_eq!(kinds(&report), [&ConflictKind::CaseOnlyRename]);
    assert_eq!(report.conflicts[0].path, "M/Poster.jpg");
    assert_eq!(
        report.conflicts[0].resolutions(),
        [ResolutionKind::Skip, ResolutionKind::Edit]
    );
    // On a case-sensitive volume they are two files.
    assert!(detect(&[copied], &MemoryDisk::new(false), &opts(false)).is_empty());
}

#[test]
fn two_names_that_differ_in_case_are_one_file_on_a_case_insensitive_volume() {
    let both = item(
        "a",
        "M",
        "M",
        FolderChange::Unchanged,
        vec![carried(
            "M/p.jpg",
            MediaFileType::Poster,
            &["M/Poster.jpg", "M/poster.jpg"],
        )],
    );
    assert_eq!(
        kinds(&detect(
            std::slice::from_ref(&both),
            &MemoryDisk::new(true),
            &opts(true)
        )),
        [&ConflictKind::CaseOnlyRename]
    );
    assert!(detect(&[both], &MemoryDisk::new(false), &opts(false)).is_empty());
}

#[test]
fn destinations_that_differ_in_case_collide_only_on_a_case_insensitive_volume() {
    let a = item(
        "a",
        "A",
        "Heat",
        FolderChange::Rename,
        vec![carried("A/x.mkv", MediaFileType::Video, &["Heat/Heat.mkv"])],
    );
    let b = item(
        "b",
        "B",
        "heat",
        FolderChange::Rename,
        vec![carried("B/y.mkv", MediaFileType::Video, &["heat/heat.mkv"])],
    );
    let items = [a, b];
    let insensitive = detect(&items, &MemoryDisk::new(true), &opts(true));
    // The folder and the video collide.
    assert_eq!(
        kinds(&insensitive),
        [
            &ConflictKind::DuplicateDestination,
            &ConflictKind::DuplicateDestination
        ]
    );
    assert!(detect(&items, &MemoryDisk::new(false), &opts(false)).is_empty());
}

// Two movies whose files differ but whose folders are one: the files would be merged.
#[test]
fn two_items_that_end_in_one_folder_collide_even_with_different_file_names() {
    let a = item(
        "a",
        "Blade Runner (1982)",
        "Blade Runner [1982]",
        FolderChange::Rename,
        vec![carried(
            "Blade Runner (1982)/br.mkv",
            MediaFileType::Video,
            &["Blade Runner [1982]/Blade Runner (1982).mkv"],
        )],
    );
    let mut b = item(
        "b",
        "Blade Runner (1982) {edition-Final Cut}",
        "Blade Runner [1982]",
        FolderChange::Rename,
        vec![carried(
            "Blade Runner (1982) {edition-Final Cut}/br.mkv",
            MediaFileType::Video,
            &["Blade Runner [1982]/Blade Runner - Final Cut (1982).mkv"],
        )],
    );
    b.base_name = Some("Blade Runner - Final Cut (1982)".to_owned());
    let disk = MemoryDisk::new(true);
    let report = detect(&[a.clone(), b.clone()], &disk, &opts(true));
    assert_eq!(kinds(&report), [&ConflictKind::DuplicateDestination]);
    assert_eq!(report.conflicts[0].path, "Blade Runner [1982]");
    assert_eq!(report.conflicts[0].items, ["a", "b"]);

    // Keeping one as a version next to the other lets them share the folder.
    let choices = BTreeMap::from([("b".to_owned(), Resolution::KeepAsVersions)]);
    let resolved = resolve(&[a.clone(), b], &choices, &disk, &opts(true)).unwrap();
    assert!(detect(&resolved.items, &disk, &opts(true)).is_empty());

    // Items that already live side by side and stay there are not a conflict.
    let mut c = a.clone();
    c.id = "c".to_owned();
    let (mut a, mut c) = (a, c);
    for item in [&mut a, &mut c] {
        item.new_folder = item.old_folder.clone();
        item.folder_change = FolderChange::Unchanged;
        item.files.clear();
    }
    assert!(detect(&[a, c], &disk, &opts(true)).is_empty());
}

#[test]
fn moving_files_into_the_folder_of_another_title_is_foreign() {
    let loose = item(
        "dune",
        "",
        "Heat (1995)",
        FolderChange::MoveFiles,
        vec![carried(
            "dune.mkv",
            MediaFileType::Video,
            &["Heat (1995)/Dune (2021).mkv"],
        )],
    );
    let disk = MemoryDisk::new(true)
        .with_dir(ROOT, "Heat (1995)", false, Some("heat"))
        .with_file(ROOT, "Heat (1995)/Heat (1995).mkv", Some("heat"));
    let report = detect(std::slice::from_ref(&loose), &disk, &opts(true));
    assert_eq!(
        kinds(&report),
        [&ConflictKind::ForeignDestination {
            owner: Some("heat".to_owned()),
            is_dir: true
        }]
    );
    // Into a shared folder it is fine.
    let mut shared = loose;
    shared.shares_folder = true;
    assert!(detect(&[shared], &disk, &opts(true)).is_empty());
}

#[test]
fn two_files_of_one_item_with_one_destination_conflict() {
    let one = item(
        "a",
        "old",
        "New",
        FolderChange::Rename,
        vec![
            carried("old/a.jpg", MediaFileType::Poster, &["New/poster.jpg"]),
            carried("old/b.jpg", MediaFileType::Poster, &["New/poster.jpg"]),
        ],
    );
    let report = detect(&[one], &MemoryDisk::new(false), &opts(false));
    assert_eq!(kinds(&report), [&ConflictKind::DuplicateDestination]);
    assert_eq!(report.conflicts[0].items, ["a"]);
}

#[test]
fn the_report_lists_the_conflicts_of_one_item() {
    let mut items = two_versions();
    items.push(planned_movie("c", "dl-3", "Heat", 1995));
    let report = detect(&items, &MemoryDisk::new(true), &opts(true));
    assert!(report.for_item("a").count() > 0);
    assert_eq!(report.for_item("c").count(), 0);
    let free = applicable(&items, &report);
    assert_eq!(free.len(), 1);
    assert_eq!(free[0].id, "c");
}

#[test]
fn native_options_match_the_platform() {
    let native = ConflictOptions::native();
    assert_eq!(
        native.case_insensitive,
        cfg!(any(windows, target_os = "macos"))
    );
    assert!(!native.allow_merge);
    assert_eq!(PathLimits::native(false).component, 255);
}

// Keep sub-folders: two movies of one category folder that render the same name still collide
// in that folder, and a folder that is already there still blocks the rename.
#[test]
fn kept_sub_folders_still_find_duplicates_and_occupied_folders() {
    let items = vec![
        planned_movie("a", "Movies/dl-1", "Les Misérables", 2012),
        planned_movie("b", "Movies/dl-2", "Les Misérables", 2012),
    ];
    assert_eq!(items[0].new_folder, "Movies/Les Misérables (2012)");
    assert_eq!(items[1].new_folder, "Movies/Les Misérables (2012)");
    let report = detect(&items, &MemoryDisk::new(true), &opts(true));
    assert!(!report.is_empty());
    assert!(
        report
            .conflicts
            .iter()
            .all(|c| c.kind == ConflictKind::DuplicateDestination)
    );

    let items = vec![planned_movie("a", "Movies/dl-1", "Heat", 1995)];
    let full = MemoryDisk::new(true).with_dir(ROOT, "Movies/Heat (1995)", false, None);
    assert_eq!(detect(&items, &full, &opts(true)).conflicts.len(), 1);
    // The same folder under the root is another place and does not block it.
    let elsewhere = MemoryDisk::new(true).with_dir(ROOT, "Heat (1995)", false, None);
    assert!(detect(&items, &elsewhere, &opts(true)).is_empty());
}
