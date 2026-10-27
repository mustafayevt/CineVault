use cv_domain::{AudioStream, Edition, Item, ItemDetail, MediaFile, MediaSource};

use super::*;
use crate::lang::SubtitleLanguageStyle;
use crate::plan::{OpKind, SubtitleTag};

fn heat() -> Item {
    let mut item = Item::movie("Heat");
    item.core.year = 1995;
    item.core.ids.set("imdb", "tt0113277");
    item.core.ids.set("tmdb", "949");
    let mut video = MediaFile::new(MediaFileType::Video, "/ds/old", "heat.mkv");
    video.width = 1920;
    video.height = 1080;
    video.audio_streams.push(AudioStream {
        codec: "DTS".to_owned(),
        language: "en".to_owned(),
        channels: 6,
        ..AudioStream::default()
    });
    item.core.media_files.push(video);
    item
}

fn file(path: &str, kind: MediaFileType) -> PlanFile {
    PlanFile::new(path, kind)
}

/// The usual set of files of a movie called `old/heat.*`.
fn files() -> Vec<PlanFile> {
    vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/heat.nfo", MediaFileType::Nfo),
        file("old/heat-poster.jpg", MediaFileType::Poster),
        file("old/fanart.jpg", MediaFileType::Fanart),
    ]
}

fn plan_with(settings: MovieSettings, item: &Item, files: &[PlanFile], folder: &str) -> MoviePlan {
    MovieRenamer::new(settings)
        .plan(&MovieInput {
            item,
            set: None,
            folder,
            files,
        })
        .expect("plans")
}

fn plan(item: &Item, files: &[PlanFile]) -> MoviePlan {
    plan_with(MovieSettings::default(), item, files, "old")
}

fn fate<'a>(plan: &'a MoviePlan, from: &str) -> &'a Fate {
    &plan
        .files
        .iter()
        .find(|f| f.from == from)
        .unwrap_or_else(|| panic!("no file {from}"))
        .fate
}

fn to(plan: &MoviePlan, from: &str) -> Vec<String> {
    match fate(plan, from) {
        Fate::Carried { to } => to.clone(),
        other => panic!("{from}: {other:?}"),
    }
}

#[test]
fn the_default_profile_renames_folder_and_files() {
    let plan = plan(&heat(), &files());
    assert_eq!(plan.new_folder, "Heat (1995)");
    assert_eq!(plan.base_name, "Heat (1995) 1080p DTS");
    assert_eq!(plan.folder_change, FolderChange::Rename);
    assert!(!plan.shared_folder);
    assert_eq!(
        to(&plan, "old/heat.mkv"),
        ["Heat (1995)/Heat (1995) 1080p DTS.mkv"]
    );
    assert_eq!(
        to(&plan, "old/heat.nfo"),
        ["Heat (1995)/Heat (1995) 1080p DTS.nfo"]
    );
    assert_eq!(
        to(&plan, "old/heat-poster.jpg"),
        ["Heat (1995)/Heat (1995) 1080p DTS-poster.jpg"]
    );
    assert_eq!(
        to(&plan, "old/fanart.jpg"),
        ["Heat (1995)/Heat (1995) 1080p DTS-fanart.jpg"]
    );
    assert!(plan.needs_rename());
    assert!(plan.problems.is_empty());
}

#[test]
fn a_tidy_movie_has_no_operations() {
    let item = heat();
    let files = vec![
        file(
            "Heat (1995)/Heat (1995) 1080p DTS.mkv",
            MediaFileType::Video,
        ),
        file("Heat (1995)/Heat (1995) 1080p DTS.nfo", MediaFileType::Nfo),
        file(
            "Heat (1995)/Heat (1995) 1080p DTS-poster.jpg",
            MediaFileType::Poster,
        ),
    ];
    let plan = plan_with(MovieSettings::default(), &item, &files, "Heat (1995)");
    assert_eq!(plan.folder_change, FolderChange::Unchanged);
    assert!(!plan.needs_rename());
    assert!(plan.ops.is_empty());
}

#[test]
fn operations_list_every_change() {
    let plan = plan(&heat(), &files());
    let moves = plan.ops.iter().filter(|op| op.kind == OpKind::Move).count();
    assert_eq!(moves, 4);
    assert!(plan.ops.iter().all(|op| op.kind == OpKind::Move));
}

#[test]
fn subtitle_suffixes_survive() {
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/old.eng.forced.srt", MediaFileType::Subtitle).subtitle(SubtitleTag {
            language: "en".to_owned(),
            forced: true,
            ..SubtitleTag::default()
        }),
        file("old/old.de.sdh.srt", MediaFileType::Subtitle).subtitle(SubtitleTag {
            language: "de".to_owned(),
            sdh: true,
            title: "Commentary".to_owned(),
            ..SubtitleTag::default()
        }),
    ];
    let settings = MovieSettings {
        file_pattern: "${title} (${year})".to_owned(),
        ..MovieSettings::default()
    };
    let plan = plan_with(settings, &heat(), &files, "old");
    assert_eq!(
        to(&plan, "old/old.eng.forced.srt"),
        ["Heat (1995)/Heat (1995).eng.forced.srt"]
    );
    assert_eq!(
        to(&plan, "old/old.de.sdh.srt"),
        ["Heat (1995)/Heat (1995).deu.sdh.Commentary.srt"]
    );
}

#[test]
fn the_index_of_a_second_track_survives_a_rename() {
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/old.eng.srt", MediaFileType::Subtitle).subtitle(SubtitleTag {
            language: "en".to_owned(),
            ..SubtitleTag::default()
        }),
        file("old/old.eng.2.srt", MediaFileType::Subtitle).subtitle(SubtitleTag {
            language: "en".to_owned(),
            title: "2".to_owned(),
            ..SubtitleTag::default()
        }),
        file("old/old.tur.forced.3.srt", MediaFileType::Subtitle).subtitle(SubtitleTag {
            language: "tr".to_owned(),
            forced: true,
            title: "3".to_owned(),
            ..SubtitleTag::default()
        }),
    ];
    let settings = MovieSettings {
        file_pattern: "${title} (${year})".to_owned(),
        ..MovieSettings::default()
    };
    let plan = plan_with(settings, &heat(), &files, "old");
    assert_eq!(
        to(&plan, "old/old.eng.srt"),
        ["Heat (1995)/Heat (1995).eng.srt"]
    );
    assert_eq!(
        to(&plan, "old/old.eng.2.srt"),
        ["Heat (1995)/Heat (1995).eng.2.srt"]
    );
    assert_eq!(
        to(&plan, "old/old.tur.forced.3.srt"),
        ["Heat (1995)/Heat (1995).tur.forced.3.srt"]
    );
}

#[test]
fn a_sub_file_takes_its_idx_along() {
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/x.sub", MediaFileType::Subtitle).subtitle(SubtitleTag {
            language: "fr".to_owned(),
            ..SubtitleTag::default()
        }),
        file("old/x.idx", MediaFileType::Unknown),
        file("old/other.idx", MediaFileType::Unknown),
    ];
    let settings = MovieSettings {
        file_pattern: "${title} (${year})".to_owned(),
        subtitle_language_style: SubtitleLanguageStyle::Iso2,
        ..MovieSettings::default()
    };
    let plan = plan_with(settings, &heat(), &files, "old");
    assert_eq!(to(&plan, "old/x.sub"), ["Heat (1995)/Heat (1995).fr.sub"]);
    assert_eq!(to(&plan, "old/x.idx"), ["Heat (1995)/Heat (1995).fr.idx"]);
    // An .idx of no subtitle is only moved along.
    assert_eq!(to(&plan, "old/other.idx"), ["Heat (1995)/other.idx"]);
}

#[test]
fn stacked_parts_keep_their_marker() {
    let files = vec![
        file("old/Heat cd1.mkv", MediaFileType::Video).stacked("cd1", 1),
        file("old/Heat cd2.mkv", MediaFileType::Video).stacked("cd2", 2),
        file("old/Heat cd1.en.srt", MediaFileType::Subtitle)
            .stacked("cd1", 1)
            .subtitle(SubtitleTag {
                language: "en".to_owned(),
                ..SubtitleTag::default()
            }),
        file("old/Heat.nfo", MediaFileType::Nfo),
    ];
    let settings = MovieSettings {
        file_pattern: "${title} (${year})".to_owned(),
        ..MovieSettings::default()
    };
    let plan = plan_with(settings, &heat(), &files, "old");
    assert_eq!(
        to(&plan, "old/Heat cd1.mkv"),
        ["Heat (1995)/Heat (1995) cd1.mkv"]
    );
    assert_eq!(
        to(&plan, "old/Heat cd2.mkv"),
        ["Heat (1995)/Heat (1995) cd2.mkv"]
    );
    assert_eq!(
        to(&plan, "old/Heat cd1.en.srt"),
        ["Heat (1995)/Heat (1995) cd1.eng.srt"]
    );
    assert_eq!(to(&plan, "old/Heat.nfo"), ["Heat (1995)/Heat (1995).nfo"]);
}

#[test]
fn space_substitution_reaches_the_stacking_marker() {
    let files = vec![file("old/Heat cd1.mkv", MediaFileType::Video).stacked("cd1", 1)];
    let settings = MovieSettings {
        file_pattern: "${title} (${year})".to_owned(),
        file_space: crate::options::SpaceSubstitution::with("."),
        ..MovieSettings::default()
    };
    let plan = plan_with(settings, &heat(), &files, "old");
    assert_eq!(
        to(&plan, "old/Heat cd1.mkv"),
        ["Heat (1995)/Heat.(1995).cd1.mkv"]
    );
}

#[test]
fn old_artwork_names_go_to_the_trash() {
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/poster.jpg", MediaFileType::Poster).modified(10),
        file("old/folder.jpg", MediaFileType::Poster).modified(20),
        file("old/cover.jpg", MediaFileType::Poster).modified(5),
    ];
    let plan = plan(&heat(), &files);
    // The newest poster is renamed; the others are moved to the trash, not deleted.
    assert_eq!(
        to(&plan, "old/folder.jpg"),
        ["Heat (1995)/Heat (1995) 1080p DTS-poster.jpg"]
    );
    assert_eq!(fate(&plan, "old/poster.jpg"), &Fate::Trash);
    assert_eq!(fate(&plan, "old/cover.jpg"), &Fate::Trash);
    let trashed: Vec<_> = plan
        .ops
        .iter()
        .filter(|op| op.kind == OpKind::Trash)
        .collect();
    assert_eq!(trashed.len(), 2);
}

#[test]
fn artwork_goes_to_every_configured_name() {
    let settings = MovieSettings {
        poster: vec!["FILENAME_POSTER".to_owned(), "POSTER".to_owned()],
        ..MovieSettings::default()
    };
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/poster.jpg", MediaFileType::Poster),
    ];
    let plan = plan_with(settings, &heat(), &files, "old");
    // The name the file has already (inside the new folder) comes first: it is moved, the
    // other name is a copy.
    assert_eq!(
        to(&plan, "old/poster.jpg"),
        [
            "Heat (1995)/poster.jpg",
            "Heat (1995)/Heat (1995) 1080p DTS-poster.jpg"
        ]
    );
    let kinds: Vec<_> = plan
        .ops
        .iter()
        .filter(|op| op.file_type == MediaFileType::Poster)
        .map(|op| op.kind)
        .collect();
    assert_eq!(kinds, [OpKind::Copy, OpKind::Move]);
}

#[test]
fn an_empty_artwork_list_leaves_the_files_alone() {
    let mut settings = MovieSettings::default();
    settings.poster.clear();
    let plan = plan_with(settings, &heat(), &files(), "old");
    assert_eq!(
        to(&plan, "old/heat-poster.jpg"),
        ["Heat (1995)/heat-poster.jpg"]
    );
}

#[test]
fn an_empty_nfo_list_trashes_the_nfo() {
    let mut settings = MovieSettings::default();
    settings.nfo.clear();
    let plan = plan_with(settings, &heat(), &files(), "old");
    assert_eq!(fate(&plan, "old/heat.nfo"), &Fate::Trash);
}

#[test]
fn nfo_names_follow_the_profile() {
    let settings = MovieSettings {
        nfo: vec!["FILENAME_NFO".to_owned(), "MOVIE_NFO".to_owned()],
        ..MovieSettings::default()
    };
    let plan = plan_with(settings, &heat(), &files(), "old");
    assert_eq!(
        to(&plan, "old/heat.nfo"),
        [
            "Heat (1995)/Heat (1995) 1080p DTS.nfo",
            "Heat (1995)/movie.nfo"
        ]
    );
}

#[test]
fn an_unreadable_nfo_stays_and_is_reported() {
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/broken.nfo", MediaFileType::Nfo).invalid_nfo(),
        file("old/heat.nfo", MediaFileType::Nfo),
    ];
    let plan = plan(&heat(), &files);
    assert_eq!(fate(&plan, "old/broken.nfo"), &Fate::LeftBehind);
    assert_eq!(
        plan.problems,
        [Problem::UnreadableNfo {
            path: "old/broken.nfo".to_owned()
        }]
    );
    // The folder cannot be renamed as a whole: the old folder keeps the file.
    assert_eq!(plan.folder_change, FolderChange::MoveFiles);

    let cleanup = MovieSettings {
        nfo_cleanup: true,
        ..MovieSettings::default()
    };
    let plan = plan_with(cleanup, &heat(), &files, "old");
    assert_eq!(fate(&plan, "old/broken.nfo"), &Fate::Trash);
    assert_eq!(plan.folder_change, FolderChange::Rename);
}

#[test]
fn the_newest_readable_nfo_wins() {
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/a.nfo", MediaFileType::Nfo).modified(1),
        file("old/b.nfo", MediaFileType::Nfo).modified(2),
    ];
    let plan = plan(&heat(), &files);
    assert_eq!(fate(&plan, "old/a.nfo"), &Fate::Trash);
    assert_eq!(
        to(&plan, "old/b.nfo"),
        ["Heat (1995)/Heat (1995) 1080p DTS.nfo"]
    );
}

#[test]
fn trailers_samples_and_sidecars_follow_the_video() {
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/heat-trailer.mp4", MediaFileType::Trailer),
        file("old/trailers/teaser.mp4", MediaFileType::Trailer),
        file("old/heat-sample.mkv", MediaFileType::Sample),
        file("old/heat-mediainfo.xml", MediaFileType::Mediainfo),
        file("old/heat.mkv.vsmeta", MediaFileType::Vsmeta),
        file("old/heat.bif", MediaFileType::Bif),
        file("old/heat-320-10.bif", MediaFileType::Bif),
        file("old/extras/making of.mkv", MediaFileType::Extra),
        file("old/heat-featurette.mkv", MediaFileType::Extra),
        file("old/readme.txt", MediaFileType::Text),
        file("old/theme.mp3", MediaFileType::Theme),
    ];
    let plan = plan(&heat(), &files);
    let base = "Heat (1995)/Heat (1995) 1080p DTS";
    assert_eq!(
        to(&plan, "old/heat-trailer.mp4"),
        [format!("{base}-trailer.mp4")]
    );
    assert_eq!(
        to(&plan, "old/trailers/teaser.mp4"),
        ["Heat (1995)/trailers/teaser.mp4"]
    );
    assert_eq!(
        to(&plan, "old/heat-sample.mkv"),
        [format!("{base}-sample.mkv")]
    );
    assert_eq!(
        to(&plan, "old/heat-mediainfo.xml"),
        [format!("{base}-mediainfo.xml")]
    );
    assert_eq!(
        to(&plan, "old/heat.mkv.vsmeta"),
        [format!("{base}.mkv.vsmeta")]
    );
    assert_eq!(to(&plan, "old/heat.bif"), [format!("{base}.bif")]);
    assert_eq!(
        to(&plan, "old/heat-320-10.bif"),
        [format!("{base}-320-10.bif")]
    );
    assert_eq!(
        to(&plan, "old/extras/making of.mkv"),
        ["Heat (1995)/extras/making of.mkv"]
    );
    assert_eq!(
        to(&plan, "old/heat-featurette.mkv"),
        [format!("{base}-featurette.mkv")]
    );
    assert_eq!(to(&plan, "old/readme.txt"), ["Heat (1995)/readme.txt"]);
    assert_eq!(to(&plan, "old/theme.mp3"), ["Heat (1995)/theme.mp3"]);
}

#[test]
fn trailer_names_come_from_the_profile() {
    let settings = MovieSettings {
        trailer: vec![
            "MOVIE_TRAILER".to_owned(),
            "TRAILERS_FILENAME_TRAILER".to_owned(),
        ],
        ..MovieSettings::default()
    };
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/a-trailer.mp4", MediaFileType::Trailer).modified(1),
        file("old/b-trailer.mp4", MediaFileType::Trailer).modified(2),
        file("old/c-trailer2.mp4", MediaFileType::Trailer),
    ];
    let plan = plan_with(settings, &heat(), &files, "old");
    assert_eq!(
        to(&plan, "old/b-trailer.mp4"),
        [
            "Heat (1995)/movie-trailer.mp4",
            "Heat (1995)/trailers/Heat (1995) 1080p DTS-trailer.mp4"
        ]
    );
    assert_eq!(fate(&plan, "old/a-trailer.mp4"), &Fate::Trash);
    // A numbered trailer keeps its number.
    assert_eq!(
        to(&plan, "old/c-trailer2.mp4"),
        [
            "Heat (1995)/movie-trailer.2.mp4",
            "Heat (1995)/trailers/Heat (1995) 1080p DTS-trailer.2.mp4"
        ]
    );
}

#[test]
fn extra_fanart_keeps_its_index() {
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/extrafanart/fanart3.jpg", MediaFileType::Extrafanart),
        file("old/extrafanart/fanart.jpg", MediaFileType::Extrafanart),
        file("old/extrafanart/a.jpg", MediaFileType::Extrafanart),
    ];
    let settings = MovieSettings {
        extra_fanart: vec!["FOLDER_EXTRAFANART".to_owned()],
        ..MovieSettings::default()
    };
    let plan = plan_with(settings, &heat(), &files, "old");
    assert_eq!(
        to(&plan, "old/extrafanart/fanart3.jpg"),
        ["Heat (1995)/extrafanart/fanart3.jpg"]
    );
    // Files without a number take the next free ones in the order of their names.
    assert_eq!(
        to(&plan, "old/extrafanart/a.jpg"),
        ["Heat (1995)/extrafanart/fanart1.jpg"]
    );
    assert_eq!(
        to(&plan, "old/extrafanart/fanart.jpg"),
        ["Heat (1995)/extrafanart/fanart2.jpg"]
    );
}

#[test]
fn the_colon_of_a_title_becomes_a_dash() {
    let mut item = Item::movie("Dune: Part Two");
    item.core.year = 2024;
    let renamer = MovieRenamer::new(MovieSettings::default());
    assert_eq!(
        renamer.folder_name(&item, None).unwrap(),
        "Dune - Part Two (2024)"
    );
}

#[test]
fn empty_brackets_of_the_detailed_pattern_are_removed() {
    let settings = MovieSettings {
        folder_pattern: "${title} (${year}) [${videoFormat}] [${mediaSource}] [${hdrformat}] [${audioCodec} ${audioChannelsDot}]" .to_owned(),
        ..MovieSettings::default()
    };
    let mut item = heat();
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.media_source = MediaSource::Bluray;
    }
    let renamer = MovieRenamer::new(settings);
    assert_eq!(
        renamer.folder_name(&item, None).unwrap(),
        "Heat (1995) [1080p] [Blu-ray] [DTS 5.1]"
    );
    // A movie without a year loses the empty parentheses too.
    item.core.year = 0;
    assert_eq!(
        renamer.folder_name(&item, None).unwrap(),
        "Heat [1080p] [Blu-ray] [DTS 5.1]"
    );
}

#[test]
fn the_edition_joins_the_default_names() {
    let mut item = heat();
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.edition = Edition::ExtendedEdition;
    }
    let renamer = MovieRenamer::new(MovieSettings::default());
    assert_eq!(
        renamer.folder_name(&item, None).unwrap(),
        "Heat - Extended Edition (1995)"
    );
}

#[test]
fn a_folder_pattern_that_is_not_unique_makes_a_shared_folder() {
    let settings = MovieSettings {
        folder_pattern: "${title;first}".to_owned(),
        poster: vec!["POSTER".to_owned()],
        nfo: vec!["MOVIE_NFO".to_owned()],
        ..MovieSettings::default()
    };
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/heat.nfo", MediaFileType::Nfo),
        file("old/poster.jpg", MediaFileType::Poster),
    ];
    let plan = plan_with(settings, &heat(), &files, "old");
    assert_eq!(plan.new_folder, "H");
    assert!(plan.shared_folder);
    assert_eq!(plan.folder_change, FolderChange::MoveFiles);
    // Files are named per movie: the fixed names of the profile would be shared by all.
    assert_eq!(to(&plan, "old/heat.nfo"), ["H/Heat (1995) 1080p DTS.nfo"]);
    assert_eq!(
        to(&plan, "old/poster.jpg"),
        ["H/Heat (1995) 1080p DTS-poster.jpg"]
    );
    assert_eq!(to(&plan, "old/heat.mkv"), ["H/Heat (1995) 1080p DTS.mkv"]);
}

#[test]
fn a_movie_leaves_a_shared_folder_for_its_own() {
    let mut item = heat();
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.multi_movie_dir = true;
    }
    let settings = MovieSettings {
        nfo: vec!["MOVIE_NFO".to_owned()],
        poster: vec!["POSTER".to_owned()],
        keep_subfolders: false,
        ..MovieSettings::default()
    };
    let files = vec![
        file("shared/heat.mkv", MediaFileType::Video),
        file("shared/heat.nfo", MediaFileType::Nfo),
        file("shared/heat-poster.jpg", MediaFileType::Poster),
    ];
    let plan = plan_with(settings, &item, &files, "shared");
    assert_eq!(plan.new_folder, "Heat (1995)");
    assert!(!plan.shared_folder);
    assert_eq!(plan.folder_change, FolderChange::MoveFiles);
    // The NFO uses the profile in the new folder; the poster still uses the video name
    // (the old folder is shared).
    assert_eq!(to(&plan, "shared/heat.nfo"), ["Heat (1995)/movie.nfo"]);
    assert_eq!(
        to(&plan, "shared/heat-poster.jpg"),
        ["Heat (1995)/Heat (1995) 1080p DTS-poster.jpg"]
    );
}

#[test]
fn a_disc_structure_keeps_its_names() {
    let mut item = heat();
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.is_disc = true;
    }
    let files = vec![
        file("old/VIDEO_TS/VIDEO_TS.IFO", MediaFileType::Video),
        file("old/VIDEO_TS/VTS_01_1.VOB", MediaFileType::Video),
        file("old/movie.nfo", MediaFileType::Nfo),
        file("old/poster.jpg", MediaFileType::Poster),
    ];
    let plan = plan(&item, &files);
    assert_eq!(
        to(&plan, "old/VIDEO_TS/VIDEO_TS.IFO"),
        ["Heat (1995)/VIDEO_TS/VIDEO_TS.IFO"]
    );
    assert_eq!(
        to(&plan, "old/VIDEO_TS/VTS_01_1.VOB"),
        ["Heat (1995)/VIDEO_TS/VTS_01_1.VOB"]
    );
    assert_eq!(to(&plan, "old/movie.nfo"), ["Heat (1995)/VIDEO_TS.nfo"]);
    // A disc prefers the fixed artwork names.
    assert_eq!(to(&plan, "old/poster.jpg"), ["Heat (1995)/poster.jpg"]);
}

#[test]
fn folder_and_file_renaming_can_be_turned_off() {
    let settings = MovieSettings {
        rename_folder: false,
        rename_files: false,
        ..MovieSettings::default()
    };
    let plan = plan_with(settings, &heat(), &files(), "old");
    assert_eq!(plan.new_folder, "old");
    assert_eq!(plan.folder_change, FolderChange::Unchanged);
    assert_eq!(plan.base_name, "heat");
    assert_eq!(to(&plan, "old/heat.mkv"), ["old/heat.mkv"]);
    // Artwork is still named from the (old) base name.
    assert_eq!(to(&plan, "old/heat-poster.jpg"), ["old/heat-poster.jpg"]);
    assert_eq!(to(&plan, "old/fanart.jpg"), ["old/heat-fanart.jpg"]);
}

#[test]
fn a_file_pattern_that_cannot_name_a_file_keeps_the_names() {
    let settings = MovieSettings {
        file_pattern: "${videoFormat}".to_owned(),
        ..MovieSettings::default()
    };
    let plan = plan_with(settings, &heat(), &files(), "old");
    assert_eq!(plan.base_name, "heat");
    assert_eq!(to(&plan, "old/heat.mkv"), ["Heat (1995)/heat.mkv"]);
}

#[test]
fn the_original_file_name_pattern_drops_the_extension() {
    let mut item = heat();
    item.core.original_filename = "Heat.1995.1080p.mkv".to_owned();
    let settings = MovieSettings {
        file_pattern: "${originalFilename}".to_owned(),
        ..MovieSettings::default()
    };
    let plan = plan_with(settings, &item, &files(), "old");
    assert_eq!(plan.base_name, "Heat.1995.1080p");
    assert_eq!(
        to(&plan, "old/heat.mkv"),
        ["Heat (1995)/Heat.1995.1080p.mkv"]
    );
}

#[test]
fn letter_folders_and_the_data_source_root() {
    let settings = MovieSettings {
        folder_pattern: "${titleSortable;first}/${title} (${year})".to_owned(),
        ..MovieSettings::default()
    };
    let files = vec![file("heat.mkv", MediaFileType::Video)];
    let plan = plan_with(settings, &heat(), &files, "");
    assert_eq!(plan.new_folder, "H/Heat (1995)");
    assert_eq!(plan.old_folder, "");
    assert_eq!(
        to(&plan, "heat.mkv"),
        ["H/Heat (1995)/Heat (1995) 1080p DTS.mkv"]
    );
    assert_eq!(plan.folder_change, FolderChange::MoveFiles);
}

#[test]
fn unwanted_files_are_trashed_when_asked() {
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/info.url", MediaFileType::Unknown),
        file("old/readme.txt", MediaFileType::Text),
        file("old/keep.dat", MediaFileType::Unknown),
    ];
    let plan_off = plan(&heat(), &files);
    assert_eq!(to(&plan_off, "old/readme.txt"), ["Heat (1995)/readme.txt"]);
    let settings = MovieSettings {
        cleanup_unwanted: true,
        ..MovieSettings::default()
    };
    let plan_on = plan_with(settings, &heat(), &files, "old");
    assert_eq!(fate(&plan_on, "old/info.url"), &Fate::Trash);
    assert_eq!(fate(&plan_on, "old/readme.txt"), &Fate::Trash);
    assert_eq!(to(&plan_on, "old/keep.dat"), ["Heat (1995)/keep.dat"]);
}

#[test]
fn two_files_at_one_destination_are_a_problem() {
    // A text file that is also named like the NFO would collide.
    let files = vec![
        file("old/heat.mkv", MediaFileType::Video),
        file("old/heat.nfo", MediaFileType::Nfo),
        file("old/Heat (1995) 1080p DTS.nfo", MediaFileType::Unknown),
    ];
    let plan = plan(&heat(), &files);
    assert_eq!(
        plan.duplicate_destinations(),
        ["Heat (1995)/Heat (1995) 1080p DTS.nfo"]
    );
    assert!(matches!(
        plan.problems.as_slice(),
        [Problem::DuplicateDestination { .. }]
    ));
}

#[test]
fn a_movie_without_a_title_is_refused() {
    let item = Item::movie("  ");
    let error = MovieRenamer::new(MovieSettings::default())
        .plan(&MovieInput {
            item: &item,
            set: None,
            folder: "x",
            files: &[],
        })
        .unwrap_err();
    assert!(matches!(error, RenameError::NoTitle));
    let show = Item::show("Show");
    let error = MovieRenamer::new(MovieSettings::default())
        .plan(&MovieInput {
            item: &show,
            set: None,
            folder: "x",
            files: &[],
        })
        .unwrap_err();
    assert!(matches!(error, RenameError::NotAMovie));
}

#[test]
fn a_pattern_that_does_not_parse_is_an_error() {
    let settings = MovieSettings {
        folder_pattern: "${title".to_owned(),
        ..MovieSettings::default()
    };
    let error = MovieRenamer::new(settings)
        .plan(&MovieInput {
            item: &heat(),
            set: None,
            folder: "old",
            files: &files(),
        })
        .unwrap_err();
    assert!(matches!(error, RenameError::Template(_)));
}

#[test]
fn the_certification_prints_as_people_read_it() {
    let mut item = heat();
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.certification = cv_domain::Certification("US_R".to_owned());
    }
    let settings = MovieSettings {
        folder_pattern: "${title} (${year}) [${certification}]".to_owned(),
        ..MovieSettings::default()
    };
    assert_eq!(
        MovieRenamer::new(settings)
            .folder_name(&item, None)
            .unwrap(),
        "Heat (1995) [R]"
    );
}

#[test]
fn a_movie_set_is_offered_for_more_than_one_movie() {
    let mut item = heat();
    item.id = Some(cv_domain::ItemId(2));
    let set = MovieSetContext {
        title: "Cops".to_owned(),
        sort_title: String::new(),
        movies: vec![1, 2],
        movies_with_dummy: vec![1, 2],
    };
    let settings = MovieSettings {
        folder_pattern: "${movieSet.title}/${title} (${year})".to_owned(),
        ..MovieSettings::default()
    };
    let renamer = MovieRenamer::new(settings);
    let plan = renamer
        .plan(&MovieInput {
            item: &item,
            set: Some(&set),
            folder: "old",
            files: &files(),
        })
        .unwrap();
    assert_eq!(plan.new_folder, "Cops/Heat (1995)");

    // A set of one movie is not offered unless the profile asks for it.
    let single = MovieSetContext {
        movies: vec![2],
        movies_with_dummy: vec![2],
        ..set
    };
    let plan = renamer
        .plan(&MovieInput {
            item: &item,
            set: Some(&single),
            folder: "old",
            files: &files(),
        })
        .unwrap();
    assert_eq!(plan.new_folder, "Heat (1995)");
}

// ---- keep sub-folders ----------------------------------------------------------------------

fn sorted_files(folder: &str) -> Vec<PlanFile> {
    vec![
        file(&format!("{folder}/heat.mkv"), MediaFileType::Video),
        file(&format!("{folder}/heat.nfo"), MediaFileType::Nfo),
    ]
}

#[test]
fn a_movie_in_a_category_folder_stays_in_it() {
    let plan = plan_with(
        MovieSettings::default(),
        &heat(),
        &sorted_files("Movies/old"),
        "Movies/old",
    );
    assert_eq!(plan.new_folder, "Movies/Heat (1995)");
    assert_eq!(plan.folder_change, FolderChange::Rename);
    assert_eq!(
        to(&plan, "Movies/old/heat.mkv"),
        ["Movies/Heat (1995)/Heat (1995) 1080p DTS.mkv"]
    );
    // Deeper sorting is kept as a whole.
    let plan = plan_with(
        MovieSettings::default(),
        &heat(),
        &sorted_files("Films/Crime/old"),
        "Films/Crime/old",
    );
    assert_eq!(plan.new_folder, "Films/Crime/Heat (1995)");
}

#[test]
fn a_renamed_movie_in_a_category_folder_is_tidy() {
    let files = vec![file(
        "Movies/Heat (1995)/Heat (1995) 1080p DTS.mkv",
        MediaFileType::Video,
    )];
    let plan = plan_with(
        MovieSettings::default(),
        &heat(),
        &files,
        "Movies/Heat (1995)",
    );
    assert_eq!(plan.new_folder, "Movies/Heat (1995)");
    assert_eq!(plan.folder_change, FolderChange::Unchanged);
    assert!(!plan.needs_rename());
}

#[test]
fn keep_sub_folders_off_renders_under_the_root() {
    let settings = MovieSettings {
        keep_subfolders: false,
        ..MovieSettings::default()
    };
    let plan = plan_with(settings, &heat(), &sorted_files("Movies/old"), "Movies/old");
    assert_eq!(plan.new_folder, "Heat (1995)");
    assert_eq!(
        to(&plan, "Movies/old/heat.mkv"),
        ["Heat (1995)/Heat (1995) 1080p DTS.mkv"]
    );
}

#[test]
fn a_pattern_with_its_own_sub_folders_starts_at_the_root() {
    let settings = MovieSettings {
        folder_pattern: "${titleSortable;first}/${title} (${year})".to_owned(),
        ..MovieSettings::default()
    };
    let plan = plan_with(
        settings.clone(),
        &heat(),
        &sorted_files("Movies/old"),
        "Movies/old",
    );
    assert_eq!(plan.new_folder, "H/Heat (1995)");
    // And a movie already there stays put: the letter folder is never nested again.
    let files = vec![file(
        "H/Heat (1995)/Heat (1995) 1080p DTS.mkv",
        MediaFileType::Video,
    )];
    let plan = plan_with(settings, &heat(), &files, "H/Heat (1995)");
    assert_eq!(plan.folder_change, FolderChange::Unchanged);
}

#[test]
fn a_movie_in_a_shared_category_folder_gets_its_own_folder_inside_it() {
    let mut item = heat();
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.multi_movie_dir = true;
    }
    let files = vec![file("Movies/heat.mkv", MediaFileType::Video)];
    let plan = plan_with(MovieSettings::default(), &item, &files, "Movies");
    assert_eq!(plan.new_folder, "Movies/Heat (1995)");
    assert_eq!(plan.folder_change, FolderChange::MoveFiles);
    assert_eq!(
        to(&plan, "Movies/heat.mkv"),
        ["Movies/Heat (1995)/Heat (1995) 1080p DTS.mkv"]
    );
    // Loose movies in the root still get their folder in the root.
    let files = vec![file("heat.mkv", MediaFileType::Video)];
    let plan = plan_with(MovieSettings::default(), &item, &files, "");
    assert_eq!(plan.new_folder, "Heat (1995)");
}

#[test]
fn a_shared_destination_keeps_its_category_folder() {
    let settings = MovieSettings {
        folder_pattern: "${title;first}".to_owned(),
        ..MovieSettings::default()
    };
    let plan = plan_with(
        settings.clone(),
        &heat(),
        &sorted_files("Movies/old"),
        "Movies/old",
    );
    assert_eq!(plan.new_folder, "Movies/H");
    assert!(plan.shared_folder);
    // A movie already in that shared folder stays in it.
    let mut item = heat();
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.multi_movie_dir = true;
    }
    let files = vec![file("Movies/H/heat.mkv", MediaFileType::Video)];
    let plan = plan_with(settings, &item, &files, "Movies/H");
    assert_eq!(plan.new_folder, "Movies/H");
    assert_eq!(plan.folder_change, FolderChange::Unchanged);
}
