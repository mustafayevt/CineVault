//! The Play action: which files of a movie or an episode go to the external player, and how.
//!
//! The files come from the item's own media files, never from a path the page sends:
//!
//! - a stacked title plays all its parts, in order of their part number (then by name);
//! - a title that is a DVD, Blu-ray or HD-DVD folder structure plays as the disc folder (the
//!   folder that holds `VIDEO_TS`, `BDMV` or `HVDVD_TS`), whichever file inside the structure
//!   the library lists;
//! - a disc image (`.iso`) plays as the file it is;
//!
//! The player comes from the settings ([`player_config`]); [`play`] hands the files and the player
//! to [`player`].
//!
//! | Key | Values | Meaning |
//! |---|---|---|
//! | `player.kind` | `system` (default), `mpv`, `vlc`, `mpc-hc`, `iina`, `custom` | the player |
//! | `player.path` | text | the player's program when it is not in its usual place |
//! | `player.custom-command` | text | the command of the `custom` player; see [`player`] |

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use cv_domain::{Item, ItemDetail, MediaFile, MediaFileType};
use cv_scan::stacking::parse_file_name;
use player::{
    DiscKind, LaunchPlan, Os, PlayInput, PlayerConfig, PlayerError, PlayerKind, launch, plan,
    program_dirs,
};
use serde_json::Value;

pub mod player;

/// The setting keys of the player.
pub const KIND_KEY: &str = "player.kind";
pub const PATH_KEY: &str = "player.path";
pub const CUSTOM_COMMAND_KEY: &str = "player.custom-command";

/// Reads the player from the settings map; anything missing or unknown takes the default (the
/// system player), so a hand-edited settings file can never stop playback.
#[must_use]
pub fn player_config(values: &BTreeMap<String, Value>) -> PlayerConfig {
    let text = |key: &str| {
        values
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    PlayerConfig {
        kind: text(KIND_KEY)
            .and_then(PlayerKind::parse)
            .unwrap_or_default(),
        program: text(PATH_KEY).map(PathBuf::from),
        custom_command: text(CUSTOM_COMMAND_KEY).unwrap_or_default().to_owned(),
    }
}

/// Why a title cannot be played.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlayError {
    /// Only movies and episodes play.
    #[error("Only movies and episodes can be played.")]
    NotPlayable,
    /// The item has no video file.
    #[error("There is no video file to play.")]
    NoVideo,
    /// A file or folder is not on disk (moved, or the drive is away).
    #[error("{0} is not on disk.")]
    FileMissing(String),
    #[error("{0}")]
    Player(#[from] PlayerError),
}

impl PlayError {
    /// A stable code for the interface.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotPlayable => "not_playable",
            Self::NoVideo => "no_video",
            Self::FileMissing(_) => "file_missing",
            Self::Player(error) => error.code(),
        }
    }
}

/// What playing did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayOutcome {
    pub player: PlayerKind,
    /// How many files (or discs) make up the title.
    pub parts: usize,
    /// How many of them the player was given. Less than `parts` only for the system player,
    /// which opens one file at a time.
    pub opened: usize,
}

/// A disc found in a file path: its folder, kind, and the folder's main index file when present.
fn disc_in(path: &Path, exists: &dyn Fn(&Path) -> bool) -> Option<PlayInput> {
    let components: Vec<Component<'_>> = path.components().collect();
    let (at, kind) = components.iter().enumerate().find_map(|(at, c)| {
        let Component::Normal(name) = c else {
            return None;
        };
        DiscKind::from_folder_name(&name.to_string_lossy()).map(|kind| (at, kind))
    })?;
    let root: PathBuf = components[..at].iter().collect();
    // A structure folder with nothing above it has no folder of its own to play.
    if root.as_os_str().is_empty() {
        return None;
    }
    let structure = root.join(components[at]);
    let index = match kind {
        DiscKind::Dvd => "VIDEO_TS.IFO",
        DiscKind::BluRay => "index.bdmv",
        DiscKind::HdDvd => "HV000I01.IFO",
    };
    let entry = Some(structure.join(index)).filter(|p| exists(p));
    Some(PlayInput::Disc { root, kind, entry })
}

/// The part number of a video file: its stored one, else the one its name carries.
fn part_number(file: &MediaFile) -> u32 {
    if file.stacking > 0 {
        file.stacking
    } else {
        parse_file_name(&file.filename).number
    }
}

/// What the Play action gives the player for `item`. `exists` tells whether a file or folder is
/// on disk.
///
/// # Errors
/// Fails when the item is not a movie or an episode, has no video, or a file is
/// missing.
pub fn play_input(item: &Item, exists: &dyn Fn(&Path) -> bool) -> Result<PlayInput, PlayError> {
    let stacked = match &item.detail {
        ItemDetail::Movie(m) => m.stacked,
        ItemDetail::Episode(e) => e.stacked,
        _ => return Err(PlayError::NotPlayable),
    };
    let mut videos: Vec<&MediaFile> = item
        .core
        .media_files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .collect();
    videos.sort_by_key(|f| {
        (
            part_number(f),
            f.directory.to_lowercase(),
            f.filename.to_lowercase(),
        )
    });
    let first = videos.first().ok_or(PlayError::NoVideo)?;

    if let Some(disc) = disc_in(&first.path(), exists) {
        if let PlayInput::Disc { root, .. } = &disc
            && !exists(root)
        {
            return Err(PlayError::FileMissing(root.display().to_string()));
        }
        return Ok(disc);
    }

    if !stacked {
        videos.truncate(1);
    }
    let mut paths = Vec::with_capacity(videos.len());
    for file in videos {
        let path = file.path();
        if !exists(&path) {
            return Err(PlayError::FileMissing(file.filename.clone()));
        }
        paths.push(path);
    }
    Ok(PlayInput::Files(paths))
}

/// Plays `item` in the configured player. `open_system` opens a file with the operating system's
/// default player (the shell supplies it).
///
/// # Errors
/// Fails when the title cannot be played ([`play_input`]) or the player cannot start.
pub fn play(
    item: &Item,
    config: &PlayerConfig,
    exists: &dyn Fn(&Path) -> bool,
    open_system: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<PlayOutcome, PlayError> {
    let input = play_input(item, exists)?;
    let parts = match &input {
        PlayInput::Files(files) => files.len(),
        PlayInput::Disc { .. } => 1,
    };
    let launch_plan = plan(config, &input, Os::current(), &program_dirs(), exists)?;
    let opened = match &launch_plan {
        LaunchPlan::System { skipped, .. } => parts.saturating_sub(*skipped),
        LaunchPlan::Program { .. } => parts,
    };
    launch(&launch_plan, open_system)?;
    Ok(PlayOutcome {
        player: config.kind,
        parts,
        opened,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    fn everything_exists(_: &Path) -> bool {
        true
    }

    fn movie(files: &[(&str, &str)], stacked: bool) -> Item {
        let mut item = Item::movie("Heat");
        if let ItemDetail::Movie(m) = &mut item.detail {
            m.stacked = stacked;
        }
        for (dir, name) in files {
            item.core
                .media_files
                .push(MediaFile::new(MediaFileType::Video, *dir, *name));
        }
        item
    }

    fn files_of(input: PlayInput) -> Vec<PathBuf> {
        match input {
            PlayInput::Files(files) => files,
            PlayInput::Disc { .. } => panic!("expected files"),
        }
    }

    fn names(files: &[PathBuf]) -> Vec<String> {
        files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_settings_choose_the_player() {
        let none = player_config(&BTreeMap::new());
        assert_eq!(none, PlayerConfig::default());
        assert_eq!(none.kind, PlayerKind::System);

        let values: BTreeMap<String, Value> = [
            (KIND_KEY.to_owned(), json!("mpv")),
            (PATH_KEY.to_owned(), json!("  D:\\Tools\\mpv.exe ")),
            (CUSTOM_COMMAND_KEY.to_owned(), json!("p {files}")),
        ]
        .into();
        let config = player_config(&values);
        assert_eq!(config.kind, PlayerKind::Mpv);
        assert_eq!(config.program, Some(PathBuf::from("D:\\Tools\\mpv.exe")));
        assert_eq!(config.custom_command, "p {files}");

        let junk: BTreeMap<String, Value> = [
            (KIND_KEY.to_owned(), json!("winamp")),
            (PATH_KEY.to_owned(), json!(7)),
        ]
        .into();
        assert_eq!(player_config(&junk), PlayerConfig::default());
    }

    #[test]
    fn a_single_file_plays_alone() {
        let item = movie(&[("/m/Heat", "Heat.mkv")], false);
        let files = files_of(play_input(&item, &everything_exists).unwrap());
        assert_eq!(files, [Path::new("/m/Heat").join("Heat.mkv")]);
    }

    #[test]
    fn stacked_parts_play_in_part_order_whatever_the_listing_order() {
        let item = movie(
            &[
                ("/m/Heat", "Heat cd2.mkv"),
                ("/m/Heat", "Heat cd3.mkv"),
                ("/m/Heat", "Heat cd1.mkv"),
            ],
            true,
        );
        let files = files_of(play_input(&item, &everything_exists).unwrap());
        assert_eq!(
            names(&files),
            ["Heat cd1.mkv", "Heat cd2.mkv", "Heat cd3.mkv"]
        );
    }

    #[test]
    fn a_stored_part_number_wins_over_the_name() {
        let mut item = movie(&[("/m", "b.mkv"), ("/m", "a.mkv")], true);
        item.core.media_files[0].stacking = 1;
        item.core.media_files[1].stacking = 2;
        let files = files_of(play_input(&item, &everything_exists).unwrap());
        assert_eq!(names(&files), ["b.mkv", "a.mkv"]);
    }

    #[test]
    fn an_unstacked_movie_plays_its_first_video_only() {
        let item = movie(&[("/m", "a.mkv"), ("/m", "b.mkv")], false);
        let files = files_of(play_input(&item, &everything_exists).unwrap());
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn a_disc_structure_plays_as_the_folder_that_holds_it() {
        let item = movie(&[("/m/Heat/VIDEO_TS", "VIDEO_TS.IFO")], false);
        let input = play_input(&item, &everything_exists).unwrap();
        assert_eq!(
            input,
            PlayInput::Disc {
                root: PathBuf::from("/m/Heat"),
                kind: DiscKind::Dvd,
                entry: Some(Path::new("/m/Heat").join("VIDEO_TS").join("VIDEO_TS.IFO")),
            }
        );
    }

    #[test]
    fn a_bluray_stream_file_plays_as_the_disc() {
        let item = movie(&[("/m/Heat/BDMV/STREAM", "00800.m2ts")], false);
        let input = play_input(&item, &|p| !p.ends_with("index.bdmv")).unwrap();
        assert_eq!(
            input,
            PlayInput::Disc {
                root: PathBuf::from("/m/Heat"),
                kind: DiscKind::BluRay,
                entry: None,
            }
        );
    }

    #[test]
    fn the_disc_folder_itself_listed_as_the_video_plays_as_the_disc() {
        let item = movie(&[("/m/Heat", "VIDEO_TS")], false);
        assert!(matches!(
            play_input(&item, &everything_exists).unwrap(),
            PlayInput::Disc {
                kind: DiscKind::Dvd,
                ..
            }
        ));
    }

    #[test]
    fn a_disc_image_plays_as_the_file() {
        let item = movie(&[("/m/Heat", "Heat.iso")], false);
        assert_eq!(
            files_of(play_input(&item, &everything_exists).unwrap()),
            [Path::new("/m/Heat").join("Heat.iso")]
        );
    }

    #[test]
    fn missing_files_and_other_items_are_refused() {
        let item = movie(&[("/m", "a.mkv"), ("/m", "b.mkv")], true);
        assert_eq!(
            play_input(&item, &|p| p.ends_with("a.mkv")),
            Err(PlayError::FileMissing("b.mkv".into()))
        );
        let disc_gone = movie(&[("/m/Heat/VIDEO_TS", "VIDEO_TS.IFO")], false);
        assert!(matches!(
            play_input(&disc_gone, &|_| false),
            Err(PlayError::FileMissing(_))
        ));
        assert_eq!(
            play_input(&movie(&[], false), &everything_exists),
            Err(PlayError::NoVideo)
        );
        assert_eq!(
            play_input(&Item::show("Show"), &everything_exists),
            Err(PlayError::NotPlayable)
        );
    }

    #[test]
    fn an_episode_plays_like_a_movie() {
        let mut item = Item::episode(cv_domain::ItemId(1), "Pilot");
        if let ItemDetail::Episode(e) = &mut item.detail {
            e.stacked = true;
        }
        for name in ["S01E01 cd2.mkv", "S01E01 cd1.mkv"] {
            item.core
                .media_files
                .push(MediaFile::new(MediaFileType::Video, "/tv/Show", name));
        }
        let files = files_of(play_input(&item, &everything_exists).unwrap());
        assert_eq!(names(&files), ["S01E01 cd1.mkv", "S01E01 cd2.mkv"]);
    }

    #[test]
    fn the_system_player_opens_the_first_part_and_says_so() {
        let item = movie(&[("/m", "a cd1.mkv"), ("/m", "a cd2.mkv")], true);
        let opened = RefCell::new(Vec::new());
        let outcome = play(&item, &PlayerConfig::default(), &everything_exists, &|p| {
            opened.borrow_mut().push(p.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(outcome.parts, 2);
        assert_eq!(outcome.opened, 1);
        assert_eq!(names(&opened.borrow()), ["a cd1.mkv"]);
    }

    #[test]
    fn a_missing_player_is_reported_with_its_code() {
        let item = movie(&[("/m", "a.mkv")], false);
        let config = PlayerConfig {
            kind: PlayerKind::Custom,
            program: None,
            custom_command: "/definitely/not/here/cv-player {files}".into(),
        };
        let error = play(&item, &config, &everything_exists, &|_| Ok(())).unwrap_err();
        assert_eq!(error.code(), "player_not_found");
    }
}
