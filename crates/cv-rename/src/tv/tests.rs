use cv_domain::{EpisodeNumber, Genre};

use super::*;
use crate::lang::SubtitleLanguageStyle;
use crate::options::SpaceSubstitution;
use crate::plan::SubtitleTag;

fn show() -> Item {
    let mut item = Item::show("Lighthouse Keepers: The Series");
    item.id = Some(ItemId(1));
    item.core.year = 2015;
    item.core.data_source = "/ds".to_owned();
    item.core.path = "/ds/Lighthouse".to_owned();
    if let ItemDetail::Show(fields) = &mut item.detail {
        fields.genres = vec![Genre::Drama];
    }
    item
}

fn episode(title: &str, season: u32, number: u32) -> Item {
    let mut item = Item::episode(ItemId(1), title);
    if let ItemDetail::Episode(fields) = &mut item.detail {
        fields.numbers.set(EpisodeNumber::aired(season, number));
    }
    item
}

fn file(path: &str, kind: MediaFileType) -> PlanFile {
    PlanFile::new(path, kind)
}

fn fate<'a>(plan: &'a TvPlan, from: &str) -> &'a Fate {
    &plan
        .files
        .iter()
        .find(|f| f.from == from)
        .unwrap_or_else(|| panic!("no file {from}"))
        .fate
}

fn to(plan: &TvPlan, from: &str) -> Vec<String> {
    match fate(plan, from) {
        Fate::Carried { to } => to.clone(),
        other => panic!("{from}: {other:?}"),
    }
}

struct Scenario {
    show: Item,
    episodes: Vec<(Item, Vec<PlanFile>)>,
    show_files: Vec<PlanFile>,
    seasons: Vec<SeasonInput>,
}

impl Scenario {
    fn new() -> Scenario {
        Scenario {
            show: show(),
            episodes: Vec::new(),
            show_files: Vec::new(),
            seasons: Vec::new(),
        }
    }

    fn with_episode(mut self, item: Item, files: Vec<PlanFile>) -> Scenario {
        self.episodes.push((item, files));
        self
    }

    fn plan(&self, settings: TvSettings) -> TvPlan {
        self.try_plan(settings).expect("plans")
    }

    fn try_plan(&self, settings: TvSettings) -> Result<TvPlan, RenameError> {
        let episodes: Vec<EpisodeInput<'_>> = self
            .episodes
            .iter()
            .map(|(item, files)| EpisodeInput { item, files })
            .collect();
        TvRenamer::new(settings).plan(&TvInput {
            show: &self.show,
            folder: "Lighthouse",
            files: &self.show_files,
            seasons: &self.seasons,
            episodes: &episodes,
        })
    }
}

fn arrival() -> Scenario {
    Scenario::new().with_episode(
        episode("Arrival", 1, 1),
        vec![
            file("Lighthouse/Season 1/arrival.mkv", MediaFileType::Video),
            file("Lighthouse/Season 1/arrival.nfo", MediaFileType::Nfo),
        ],
    )
}

#[test]
fn folders_and_names_follow_the_default_patterns() {
    let plan = arrival().plan(TvSettings::default());
    assert_eq!(plan.new_folder, "Lighthouse Keepers The Series (2015)");
    assert_eq!(plan.folder_change, FolderChange::Rename);
    let base = "Lighthouse Keepers The Series (2015)/Season 1/Lighthouse Keepers The Series - S01E01 - Arrival";
    assert_eq!(
        to(&plan, "Lighthouse/Season 1/arrival.mkv"),
        [format!("{base}.mkv")]
    );
    assert_eq!(
        to(&plan, "Lighthouse/Season 1/arrival.nfo"),
        [format!("{base}.nfo")]
    );
    assert!(plan.needs_rename());
}

/// reg2-7: an episode the scan found no title for carries its number marker as the title
/// (`S01E01`). The name leaves the title part out instead of repeating the number.
#[test]
fn a_title_that_only_restates_the_episode_number_is_left_out_of_the_name() {
    for title in ["S01E01", "s01e01", "S01 E01", "1x01", "01x01", "S01E01E02"] {
        let plan = Scenario::new()
            .with_episode(
                episode(title, 1, 1),
                vec![file(
                    "Lighthouse/Season 1/Dark S01E01.mkv",
                    MediaFileType::Video,
                )],
            )
            .plan(TvSettings::default());
        assert_eq!(
            to(&plan, "Lighthouse/Season 1/Dark S01E01.mkv"),
            [
                "Lighthouse Keepers The Series (2015)/Season 1/Lighthouse Keepers The Series - S01E01.mkv"
            ],
            "{title}"
        );
    }
    // A real title that holds a number is kept.
    let plan = Scenario::new()
        .with_episode(
            episode("S01E01 Revisited", 1, 1),
            vec![file("Lighthouse/Season 1/a.mkv", MediaFileType::Video)],
        )
        .plan(TvSettings::default());
    assert_eq!(
        to(&plan, "Lighthouse/Season 1/a.mkv"),
        [
            "Lighthouse Keepers The Series (2015)/Season 1/Lighthouse Keepers The Series - S01E01 - S01E01 Revisited.mkv"
        ]
    );
}

#[test]
fn an_episode_without_numbers_keeps_its_name_and_place() {
    let mut bonus = Item::episode(ItemId(1), "Behind the Scenes Interview");
    if let ItemDetail::Episode(fields) = &mut bonus.detail {
        fields.numbers = Default::default();
    }
    let scenario = arrival().with_episode(
        bonus,
        vec![
            file(
                "Lighthouse/Season 2/Behind the Scenes Interview.mkv",
                MediaFileType::Video,
            ),
            file(
                "Lighthouse/Season 2/Behind the Scenes Interview.srt",
                MediaFileType::Subtitle,
            ),
        ],
    );
    let plan = scenario.plan(TvSettings::default());
    let folder = "Lighthouse Keepers The Series (2015)";
    assert_eq!(
        to(&plan, "Lighthouse/Season 2/Behind the Scenes Interview.mkv"),
        [format!("{folder}/Season 2/Behind the Scenes Interview.mkv")]
    );
    assert_eq!(
        to(&plan, "Lighthouse/Season 2/Behind the Scenes Interview.srt"),
        [format!("{folder}/Season 2/Behind the Scenes Interview.srt")]
    );
    for planned in &plan.files {
        if let Fate::Carried { to } = &planned.fate {
            assert!(
                to.iter().all(|t| !t.contains("-1")),
                "a missing number never reaches a name: {to:?}"
            );
        }
    }
}

#[test]
fn season_zero_is_the_specials_folder() {
    let scenario = Scenario::new().with_episode(
        episode("Behind the Light", 0, 1),
        vec![file("Lighthouse/extra/b.mkv", MediaFileType::Video)],
    );
    let plan = scenario.plan(TvSettings::default());
    assert_eq!(
        to(&plan, "Lighthouse/extra/b.mkv"),
        [
            "Lighthouse Keepers The Series (2015)/Specials/Lighthouse Keepers The Series - S00E01 - Behind the Light.mkv"
        ]
    );
    let off = TvSettings {
        special_season: false,
        ..TvSettings::default()
    };
    let plan = scenario.plan(off);
    assert!(to(&plan, "Lighthouse/extra/b.mkv")[0].contains("/Season 0/"));
}

#[test]
fn an_empty_season_pattern_is_the_show_folder_for_a_recommended_pair() {
    let settings = TvSettings {
        season_pattern: String::new(),
        ..TvSettings::default()
    };
    let scenario = arrival();
    let plan = scenario.plan(settings.clone());
    assert!(
        to(&plan, "Lighthouse/Season 1/arrival.mkv")[0]
            .starts_with("Lighthouse Keepers The Series (2015)/Lighthouse Keepers")
    );
    assert!(TvRenamer::new(settings).is_recommended());
}

#[test]
fn an_empty_season_pattern_falls_back_to_season_n_when_the_pair_is_not_recommended() {
    // The season only comes from the folder, and the folder pattern is empty: the file names
    // would not tell the seasons apart.
    let settings = TvSettings {
        season_pattern: String::new(),
        file_pattern: "${showTitle} - E${episodeNr2} - ${title}".to_owned(),
        ..TvSettings::default()
    };
    assert!(!TvRenamer::new(settings.clone()).is_recommended());
    let scenario = Scenario::new()
        .with_episode(
            episode("Arrival", 1, 1),
            vec![file("Lighthouse/a.mkv", MediaFileType::Video)],
        )
        .with_episode(
            episode("Watch", 12, 1),
            vec![file("Lighthouse/b.mkv", MediaFileType::Video)],
        )
        .with_episode(
            episode("Special", 0, 1),
            vec![file("Lighthouse/c.mkv", MediaFileType::Video)],
        );
    let plan = scenario.plan(settings);
    let first = &to(&plan, "Lighthouse/a.mkv")[0];
    assert!(first.contains("/Season 1/"), "{first}");
    assert!(to(&plan, "Lighthouse/b.mkv")[0].contains("/Season 12/"));
    assert!(to(&plan, "Lighthouse/c.mkv")[0].contains("/Specials/"));
}

#[test]
fn a_file_pattern_without_an_episode_token_is_refused() {
    let settings = TvSettings {
        file_pattern: "${showTitle} - ${title}".to_owned(),
        ..TvSettings::default()
    };
    let error = arrival().try_plan(settings).unwrap_err();
    assert!(matches!(error, RenameError::EpisodePatternWithoutEpisode));
    // Keeping the original name needs no token; neither does leaving the files alone.
    let original = TvSettings {
        file_pattern: "${originalBasename}".to_owned(),
        ..TvSettings::default()
    };
    assert!(arrival().try_plan(original).is_ok());
    let off = TvSettings {
        file_pattern: "static".to_owned(),
        rename_files: false,
        ..TvSettings::default()
    };
    assert!(arrival().try_plan(off).is_ok());
}

#[test]
fn episodes_of_one_file_are_named_together() {
    let renamer = TvRenamer::new(TvSettings::default());
    let show = show();
    let one = episode("Storm Warning", 1, 2);
    let two = episode("Storm Warning Part Two", 1, 3);
    let name = renamer.episode_file_name(&show, &[&one, &two]).unwrap();
    assert_eq!(
        name,
        "Lighthouse Keepers The Series - S01E02 S01E03 - Storm Warning - Storm Warning Part Two"
    );
    let range = TvSettings {
        multi_episode_style: MultiEpisodeStyle::Range,
        ..TvSettings::default()
    };
    let name = TvRenamer::new(range)
        .episode_file_name(&show, &[&one, &two])
        .unwrap();
    assert_eq!(
        name,
        "Lighthouse Keepers The Series - S01E02-E03 - Storm Warning - Storm Warning Part Two"
    );
}

#[test]
fn a_range_needs_consecutive_episodes_of_one_season() {
    let range = TvSettings {
        multi_episode_style: MultiEpisodeStyle::Range,
        file_pattern: "${showTitle} - S${seasonNr2}E${episodeNr2}".to_owned(),
        ..TvSettings::default()
    };
    let renamer = TvRenamer::new(range);
    let show = show();
    let e1 = episode("A", 1, 1);
    let e3 = episode("B", 1, 3);
    let s2 = episode("C", 2, 1);
    let name = |episodes: &[&Item]| renamer.episode_file_name(&show, episodes).unwrap();
    assert_eq!(
        name(&[&e1, &e3]),
        "Lighthouse Keepers The Series - S01E01 S01E03"
    );
    assert_eq!(
        name(&[&e3, &s2]),
        "Lighthouse Keepers The Series - S01E03 S02E01"
    );
    let e2 = episode("D", 1, 2);
    assert_eq!(
        name(&[&e1, &e2, &e3]),
        "Lighthouse Keepers The Series - S01E01-E03"
    );
}

#[test]
fn a_multi_episode_file_is_planned_once() {
    // Two episodes share the video; the third is a separate file.
    let plan = Scenario::new()
        .with_episode(
            episode("One", 1, 2),
            vec![file("Lighthouse/ep23.mkv", MediaFileType::Video)],
        )
        .with_episode(
            episode("Two", 1, 3),
            vec![file("Lighthouse/ep23.mkv", MediaFileType::Video)],
        )
        .plan(TvSettings::default());
    assert_eq!(plan.files.len(), 1);
    assert_eq!(
        to(&plan, "Lighthouse/ep23.mkv"),
        [
            "Lighthouse Keepers The Series (2015)/Season 1/Lighthouse Keepers The Series - S01E02 S01E03 - One - Two.mkv"
        ]
    );
}

#[test]
fn the_other_files_of_an_episode_follow_its_name() {
    let base = "Lighthouse Keepers The Series (2015)/Season 1/Lighthouse Keepers The Series - S01E01 - Arrival";
    let files = vec![
        file("Lighthouse/Season 1/a.mkv", MediaFileType::Video),
        file("Lighthouse/Season 1/a-thumb.jpg", MediaFileType::Thumb),
        file("Lighthouse/Season 1/a.en.srt", MediaFileType::Subtitle).subtitle(SubtitleTag {
            language: "en".to_owned(),
            forced: true,
            ..SubtitleTag::default()
        }),
        file("Lighthouse/Season 1/a.sub", MediaFileType::Subtitle),
        file("Lighthouse/Season 1/a.idx", MediaFileType::Unknown),
        file("Lighthouse/Season 1/a-trailer.mp4", MediaFileType::Trailer),
        file("Lighthouse/Season 1/a-sample.mkv", MediaFileType::Sample),
        file("Lighthouse/Season 1/a-fanart.jpg", MediaFileType::Fanart),
        file("Lighthouse/Season 1/a.mkv.vsmeta", MediaFileType::Vsmeta),
        file(
            "Lighthouse/Season 1/a-mediainfo.xml",
            MediaFileType::Mediainfo,
        ),
        file("Lighthouse/Season 1/a-extra.mkv", MediaFileType::Extra),
        file("Lighthouse/Season 1/a.txt", MediaFileType::Text),
        file("Lighthouse/Season 1/other.txt", MediaFileType::Text),
        file("Lighthouse/Season 1/extras/clip.mkv", MediaFileType::Extra),
        file("Lighthouse/Season 1/a-poster.jpg", MediaFileType::Poster),
    ];
    let scenario = Scenario::new().with_episode(episode("Arrival", 1, 1), files);
    let plan = scenario.plan(TvSettings::default());
    let at = |from: &str| to(&plan, &format!("Lighthouse/Season 1/{from}"));
    assert_eq!(at("a.mkv"), [format!("{base}.mkv")]);
    assert_eq!(at("a-thumb.jpg"), [format!("{base}-thumb.jpg")]);
    assert_eq!(at("a.en.srt"), [format!("{base}.eng.forced.srt")]);
    assert_eq!(at("a.sub"), [format!("{base}.sub")]);
    assert_eq!(at("a.idx"), [format!("{base}.idx")]);
    assert_eq!(at("a-trailer.mp4"), [format!("{base}-trailer.mp4")]);
    assert_eq!(at("a-sample.mkv"), [format!("{base}-sample.mkv")]);
    assert_eq!(at("a-fanart.jpg"), [format!("{base}-fanart.jpg")]);
    assert_eq!(at("a.mkv.vsmeta"), [format!("{base}.mkv.vsmeta")]);
    assert_eq!(at("a-mediainfo.xml"), [format!("{base}-mediainfo.xml")]);
    assert_eq!(at("a-extra.mkv"), [format!("{base}-extra.mkv")]);
    assert_eq!(at("a.txt"), [format!("{base}.txt")]);
    // Files that do not start with the old name, and files in sub-folders, move along.
    assert_eq!(
        at("other.txt"),
        ["Lighthouse Keepers The Series (2015)/Season 1/other.txt"]
    );
    assert_eq!(
        at("extras/clip.mkv"),
        ["Lighthouse Keepers The Series (2015)/Season 1/extras/clip.mkv"]
    );
    // Other artwork is not an episode file and is left in place.
    assert_eq!(
        at("a-poster.jpg"),
        ["Lighthouse Keepers The Series (2015)/Season 1/a-poster.jpg"]
    );
}

#[test]
fn episode_thumbnails_use_the_names_of_the_profile() {
    let settings = TvSettings {
        episode_thumb: vec!["FILENAME_TBN".to_owned(), "FILENAME".to_owned()],
        ..TvSettings::default()
    };
    let scenario = Scenario::new().with_episode(
        episode("Arrival", 1, 1),
        vec![
            file("Lighthouse/a.mkv", MediaFileType::Video),
            file("Lighthouse/a-thumb.jpg", MediaFileType::Thumb).modified(2),
            file("Lighthouse/a.jpg", MediaFileType::Thumb).modified(1),
        ],
    );
    let plan = scenario.plan(settings);
    let base = "Lighthouse Keepers The Series (2015)/Season 1/Lighthouse Keepers The Series - S01E01 - Arrival";
    assert_eq!(
        to(&plan, "Lighthouse/a-thumb.jpg"),
        [format!("{base}.tbn"), format!("{base}.jpg")]
    );
    assert_eq!(fate(&plan, "Lighthouse/a.jpg"), &Fate::Trash);
}

#[test]
fn show_and_season_artwork_follow_the_profile() {
    let mut scenario = arrival();
    scenario.show_files = vec![
        file("Lighthouse/poster.jpg", MediaFileType::Poster),
        file("Lighthouse/tvshow.nfo", MediaFileType::Nfo),
        file("Lighthouse/theme.mp3", MediaFileType::Theme),
        file(
            "Lighthouse/season-specials-poster.jpg",
            MediaFileType::SeasonPoster,
        ),
    ];
    scenario.seasons = vec![SeasonInput {
        season: 1,
        files: vec![
            file(
                "Lighthouse/season01-poster.jpg",
                MediaFileType::SeasonPoster,
            ),
            file(
                "Lighthouse/season01-fanart.jpg",
                MediaFileType::SeasonFanart,
            ),
        ],
    }];
    let settings = TvSettings {
        poster: vec!["POSTER".to_owned(), "FOLDER".to_owned()],
        season_poster: vec!["SEASON_POSTER".to_owned(), "SEASON_FOLDER".to_owned()],
        ..TvSettings::default()
    };
    let plan = scenario.plan(settings);
    let root = "Lighthouse Keepers The Series (2015)";
    assert_eq!(
        to(&plan, "Lighthouse/poster.jpg"),
        [format!("{root}/poster.jpg"), format!("{root}/folder.jpg")]
    );
    assert_eq!(
        to(&plan, "Lighthouse/tvshow.nfo"),
        [format!("{root}/tvshow.nfo")]
    );
    assert_eq!(
        to(&plan, "Lighthouse/theme.mp3"),
        [format!("{root}/theme.mp3")]
    );
    assert_eq!(
        to(&plan, "Lighthouse/season-specials-poster.jpg"),
        [format!("{root}/season-specials-poster.jpg")]
    );
    assert_eq!(
        to(&plan, "Lighthouse/season01-poster.jpg"),
        [
            format!("{root}/season01-poster.jpg"),
            format!("{root}/Season 1/season01-poster.jpg")
        ]
    );
    assert_eq!(
        to(&plan, "Lighthouse/season01-fanart.jpg"),
        [format!("{root}/season01-fanart.jpg")]
    );
    let owners: Vec<Owner> = plan.files.iter().map(|f| f.owner).collect();
    assert!(owners.contains(&Owner::Show));
    assert!(owners.contains(&Owner::Season(1)));
    assert!(owners.contains(&Owner::Episodes(0)));
}

#[test]
fn season_nfos_use_the_season_folder_when_asked() {
    let mut scenario = arrival();
    scenario.seasons = vec![SeasonInput {
        season: 1,
        files: vec![file("Lighthouse/season01.nfo", MediaFileType::Nfo)],
    }];
    let settings = TvSettings {
        season_nfo: vec!["SEASON_FOLDER".to_owned()],
        ..TvSettings::default()
    };
    let plan = scenario.plan(settings);
    assert_eq!(
        to(&plan, "Lighthouse/season01.nfo"),
        ["Lighthouse Keepers The Series (2015)/Season 1/season.nfo"]
    );
}

#[test]
fn folders_and_files_can_be_left_alone() {
    let settings = TvSettings {
        rename_show_folder: false,
        rename_season_folder: false,
        rename_files: false,
        ..TvSettings::default()
    };
    let plan = arrival().plan(settings);
    assert_eq!(plan.new_folder, "Lighthouse");
    assert_eq!(plan.folder_change, FolderChange::Unchanged);
    assert_eq!(
        to(&plan, "Lighthouse/Season 1/arrival.mkv"),
        ["Lighthouse/Season 1/arrival.mkv"]
    );
    assert_eq!(
        to(&plan, "Lighthouse/Season 1/arrival.nfo"),
        ["Lighthouse/Season 1/arrival.nfo"]
    );
    assert!(!plan.needs_rename());
}

#[test]
fn space_substitution_applies_per_part() {
    let settings = TvSettings {
        show_space: SpaceSubstitution::with("."),
        season_space: SpaceSubstitution::with("_"),
        file_space: SpaceSubstitution::with("-"),
        subtitle_language_style: SubtitleLanguageStyle::Iso2,
        ..TvSettings::default()
    };
    let plan = arrival().plan(settings);
    assert_eq!(
        to(&plan, "Lighthouse/Season 1/arrival.mkv"),
        [
            "Lighthouse.Keepers.The.Series.(2015)/Season_1/Lighthouse-Keepers-The-Series-S01E01-Arrival.mkv"
        ]
    );
}

#[test]
fn the_certification_and_status_tokens_are_readable() {
    let mut show = show();
    if let ItemDetail::Show(fields) = &mut show.detail {
        fields.certification = cv_domain::Certification("US_TVPG".to_owned());
        fields.status = cv_domain::AiredStatus::Continuing;
    }
    let renamer = TvRenamer::new(TvSettings::default());
    assert_eq!(
        renamer
            .show_folder_name_for("${showTitle} [${showCertification}] ${showStatus}", &show)
            .unwrap(),
        "Lighthouse Keepers The Series [TV-PG] Continuing"
    );
}

#[test]
fn a_show_without_a_title_or_of_another_kind_is_refused() {
    let mut scenario = Scenario::new();
    scenario.show = Item::show("");
    assert!(matches!(
        scenario.try_plan(TvSettings::default()).unwrap_err(),
        RenameError::NoTitle
    ));
    scenario.show = Item::movie("Heat");
    assert!(matches!(
        scenario.try_plan(TvSettings::default()).unwrap_err(),
        RenameError::NotAShow
    ));
}

#[test]
fn tv_pattern_pairs_are_checked_for_the_fallback() {
    let renamer = TvRenamer::new(TvSettings::default());
    let check = |season: Option<&str>, file: &str| renamer.is_recommended_for(season, file);
    assert!(check(
        Some("Season ${seasonNr}"),
        "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}"
    ));
    assert!(check(
        Some(""),
        "${showTitle} - S${seasonNr2}E${episodeNr2}"
    ));
    assert!(check(Some("Season ${seasonNr}"), "${originalFilename}"));
    // Two titles are only advice.
    assert!(check(
        Some("Season ${seasonNr}"),
        "S${seasonNr2}E${episodeNr2} ${title} ${title}"
    ));
    assert!(!check(
        Some("Season ${seasonNr}"),
        "E${episodeNr2}S${seasonNr2}"
    ));
    assert!(!check(Some("Season ${seasonNr}"), "${showTitle}"));
    assert!(!check(
        Some("Season ${seasonNr}"),
        "S${seasonNr2}E${episodeNr2}E${episodeNr}"
    ));
}

// ---- keep sub-folders ----------------------------------------------------------------------

fn plan_in(folder: &str, settings: TvSettings) -> TvPlan {
    let show = show();
    let ep = episode("Arrival", 1, 1);
    let files = vec![file(
        &format!("{folder}/Season 1/arrival.mkv"),
        MediaFileType::Video,
    )];
    let episodes = vec![EpisodeInput {
        item: &ep,
        files: &files,
    }];
    TvRenamer::new(settings)
        .plan(&TvInput {
            show: &show,
            folder,
            files: &[],
            seasons: &[],
            episodes: &episodes,
        })
        .expect("plans")
}

#[test]
fn a_show_in_a_category_folder_stays_in_it() {
    let plan = plan_in("TV/Anime/Lighthouse", TvSettings::default());
    assert_eq!(
        plan.new_folder,
        "TV/Anime/Lighthouse Keepers The Series (2015)"
    );
    assert_eq!(plan.folder_change, FolderChange::Rename);
    assert_eq!(
        to(&plan, "TV/Anime/Lighthouse/Season 1/arrival.mkv"),
        [
            "TV/Anime/Lighthouse Keepers The Series (2015)/Season 1/Lighthouse Keepers The Series - S01E01 - Arrival.mkv"
        ]
    );
}

#[test]
fn a_show_with_keep_sub_folders_off_goes_to_the_root() {
    let settings = TvSettings {
        keep_subfolders: false,
        ..TvSettings::default()
    };
    let plan = plan_in("TV/Anime/Lighthouse", settings);
    assert_eq!(plan.new_folder, "Lighthouse Keepers The Series (2015)");
}

#[test]
fn a_show_pattern_with_sub_folders_starts_at_the_root() {
    let settings = TvSettings {
        show_pattern: "${showTitle;first}/${showTitle} (${showYear})".to_owned(),
        ..TvSettings::default()
    };
    let plan = plan_in("TV/Anime/Lighthouse", settings);
    assert_eq!(plan.new_folder, "L/Lighthouse Keepers The Series (2015)");
}
