//! External player launch: which program plays a title, with which arguments, and starting it.
//!
//! The Play action hands a [`PlayInput`] (the video files of a title in play order, or a disc
//! folder) and the user's [`PlayerConfig`] to [`plan`], which answers with a [`LaunchPlan`]:
//!
//! - the system default player opens the (first) file through the operating system;
//! - mpv, VLC, MPC-HC/BE and IINA start their program with every part on the command line, in
//!   order, so the player queues the parts as one playlist;
//! - a custom command is a template whose placeholders are replaced.
//!
//! Planning is pure: nothing is started and no file is looked at, except through the `exists`
//! probe a caller hands to [`resolve_program`]. [`launch`] then starts the program. No shell is
//! involved anywhere: the program and every argument are separate strings handed to the
//! operating system, so a file name can never be read as a command.
//!
//! # Custom commands
//!
//! The first word is the program, the rest are arguments. Words are split on spaces; double or
//! single quotes group words that contain spaces (a quote cannot be escaped). Placeholders:
//!
//! | Placeholder | Replaced by |
//! |---|---|
//! | `{file}` | the first file (the disc folder for a disc) |
//! | `{files}` | every file in play order, as separate arguments; it must stand alone as a word |
//! | `{folder}` | the folder holding the first file (the disc folder itself for a disc) |
//!
//! A command with no placeholder gets `{files}` added at the end.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Which player starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PlayerKind {
    /// Whatever the operating system opens the file with.
    #[default]
    System,
    Mpv,
    Vlc,
    /// MPC-HC or MPC-BE (they share their command line).
    MpcHc,
    Iina,
    /// The user's own command template.
    Custom,
}

impl PlayerKind {
    /// Every kind, in the order the settings page lists them.
    pub const ALL: [Self; 6] = [
        Self::System,
        Self::Mpv,
        Self::Vlc,
        Self::MpcHc,
        Self::Iina,
        Self::Custom,
    ];

    /// The stable text used in the settings file.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Mpv => "mpv",
            Self::Vlc => "vlc",
            Self::MpcHc => "mpc-hc",
            Self::Iina => "iina",
            Self::Custom => "custom",
        }
    }

    /// Parses the text produced by [`PlayerKind::as_str`].
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == text)
    }

    /// The name shown to the user.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System default",
            Self::Mpv => "mpv",
            Self::Vlc => "VLC",
            Self::MpcHc => "MPC-HC / MPC-BE",
            Self::Iina => "IINA",
            Self::Custom => "Custom command",
        }
    }
}

/// The kind of a disc structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscKind {
    Dvd,
    BluRay,
    HdDvd,
}

impl DiscKind {
    /// The kind a folder name stands for (`VIDEO_TS`, `BDMV`, `HVDVD_TS`), ignoring case.
    #[must_use]
    pub fn from_folder_name(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("video_ts") {
            Some(Self::Dvd)
        } else if name.eq_ignore_ascii_case("bdmv") {
            Some(Self::BluRay)
        } else if name.eq_ignore_ascii_case("hvdvd_ts") {
            Some(Self::HdDvd)
        } else {
            None
        }
    }
}

/// What to play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayInput {
    /// Video files (or disc images) in play order. Never empty.
    Files(Vec<PathBuf>),
    /// A disc kept as its folder structure.
    Disc {
        /// The folder that holds `VIDEO_TS` / `BDMV` / `HVDVD_TS`.
        root: PathBuf,
        kind: DiscKind,
        /// The disc's main index file (`VIDEO_TS.IFO`, `index.bdmv`): the one thing the system
        /// default player can be asked to open for a disc.
        entry: Option<PathBuf>,
    },
}

/// The user's player choice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerConfig {
    pub kind: PlayerKind,
    /// The player's program when it is not installed in a usual place. Empty means "find it".
    pub program: Option<PathBuf>,
    /// The template of [`PlayerKind::Custom`].
    pub custom_command: String,
}

/// What [`plan`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchPlan {
    /// The operating system opens this file with its default player.
    System {
        file: PathBuf,
        /// Files of the title that were not opened (the system opens one file at a time).
        skipped: usize,
    },
    /// A program is started with these arguments.
    Program {
        program: PathBuf,
        args: Vec<OsString>,
    },
}

/// Why a title cannot be played.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlayerError {
    #[error(
        "The system default player cannot open a disc folder. Choose mpv, VLC, MPC-HC or IINA in the settings."
    )]
    SystemCannotPlayDisc,
    #[error("The custom player command is empty. Enter a command in the settings.")]
    EmptyCustomCommand,
    #[error("The custom player command has a quote that is never closed.")]
    UnbalancedQuote,
    #[error("In the custom player command, {{files}} has to be a word of its own.")]
    FilesPlaceholderNotAlone,
    #[error("The player {0} was not found. Check its location in the settings.")]
    NotFound(String),
    #[error("The player could not be started: {0}")]
    Start(String),
    #[error("The system could not open the file: {0}")]
    System(String),
}

impl PlayerError {
    /// A stable code for the interface.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SystemCannotPlayDisc => "system_cannot_play_disc",
            Self::EmptyCustomCommand => "empty_custom_command",
            Self::UnbalancedQuote | Self::FilesPlaceholderNotAlone => "bad_custom_command",
            Self::NotFound(_) => "player_not_found",
            Self::Start(_) | Self::System(_) => "launch_failed",
        }
    }
}

/// The operating system families that matter for where players are installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Other,
}

impl Os {
    /// The system this build runs on.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Other
        }
    }
}

/// The folders programs are installed in on Windows (`ProgramFiles`, `ProgramW6432`,
/// `ProgramFiles(x86)`), without duplicates. Empty elsewhere.
#[must_use]
pub fn program_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for name in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(value) = std::env::var_os(name) {
            let dir = PathBuf::from(value);
            if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs
}

/// The places a player is usually installed in, best first.
#[must_use]
pub fn known_locations(kind: PlayerKind, os: Os, program_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let under = |relative: &str| -> Vec<PathBuf> {
        program_dirs.iter().map(|dir| dir.join(relative)).collect()
    };
    match (os, kind) {
        (Os::Windows, PlayerKind::Mpv) => under("mpv\\mpv.exe"),
        (Os::Windows, PlayerKind::Vlc) => under("VideoLAN\\VLC\\vlc.exe"),
        (Os::Windows, PlayerKind::MpcHc) => [
            "MPC-HC\\mpc-hc64.exe",
            "MPC-HC\\mpc-hc.exe",
            "MPC-BE x64\\mpc-be64.exe",
            "MPC-BE\\mpc-be.exe",
            "K-Lite Codec Pack\\MPC-HC64\\mpc-hc64.exe",
        ]
        .into_iter()
        .flat_map(under)
        .collect(),
        (Os::MacOs, PlayerKind::Mpv) => vec![
            "/opt/homebrew/bin/mpv".into(),
            "/usr/local/bin/mpv".into(),
            "/Applications/mpv.app/Contents/MacOS/mpv".into(),
        ],
        (Os::MacOs, PlayerKind::Vlc) => vec!["/Applications/VLC.app/Contents/MacOS/VLC".into()],
        (Os::MacOs, PlayerKind::Iina) => {
            vec!["/Applications/IINA.app/Contents/MacOS/iina-cli".into()]
        }
        _ => Vec::new(),
    }
}

/// The program name to look for on the `PATH` when no usual location holds the player.
const fn path_name(kind: PlayerKind, os: Os) -> &'static str {
    match (kind, os) {
        (PlayerKind::Vlc, _) => "vlc",
        (PlayerKind::MpcHc, Os::Windows) => "mpc-hc64",
        (PlayerKind::MpcHc, _) => "mpc-hc",
        (PlayerKind::Iina, _) => "iina-cli",
        _ => "mpv",
    }
}

/// The program of a player: the configured one when set, else the first usual location that
/// `exists`, else its name for a `PATH` search.
///
/// A configured macOS application bundle (`VLC.app`) stands for the program inside it.
#[must_use]
pub fn resolve_program(
    kind: PlayerKind,
    configured: Option<&Path>,
    os: Os,
    program_dirs: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> PathBuf {
    if let Some(path) = configured.filter(|p| !p.as_os_str().is_empty()) {
        return inside_app_bundle(kind, path);
    }
    known_locations(kind, os, program_dirs)
        .into_iter()
        .find(|candidate| exists(candidate))
        .unwrap_or_else(|| PathBuf::from(path_name(kind, os)))
}

fn inside_app_bundle(kind: PlayerKind, path: &Path) -> PathBuf {
    let is_bundle = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"));
    if !is_bundle {
        return path.to_path_buf();
    }
    let program = if kind == PlayerKind::Iina {
        "iina-cli".to_owned()
    } else {
        path.file_stem()
            .map_or_else(|| "mpv".to_owned(), |s| s.to_string_lossy().into_owned())
    };
    path.join("Contents").join("MacOS").join(program)
}

/// A path as a command-line word. A relative name that starts with `-` is made explicit so a
/// player cannot read it as an option.
fn word(path: &Path) -> OsString {
    if path.to_string_lossy().starts_with('-') {
        let mut explicit = OsString::from("./");
        explicit.push(path);
        explicit
    } else {
        path.as_os_str().to_owned()
    }
}

/// Decides how `input` is played with `config`. Nothing is started.
///
/// `os`, `program_dirs` and `exists` only matter for finding an installed player; see
/// [`resolve_program`].
///
/// # Errors
/// Fails when the system default player is asked for a disc folder, or when the custom command
/// is empty or malformed.
pub fn plan(
    config: &PlayerConfig,
    input: &PlayInput,
    os: Os,
    program_dirs: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> Result<LaunchPlan, PlayerError> {
    let locate = |kind| resolve_program(kind, config.program.as_deref(), os, program_dirs, exists);
    match config.kind {
        PlayerKind::System => match input {
            PlayInput::Files(files) => Ok(LaunchPlan::System {
                file: files.first().cloned().unwrap_or_default(),
                skipped: files.len().saturating_sub(1),
            }),
            PlayInput::Disc {
                entry: Some(entry), ..
            } => Ok(LaunchPlan::System {
                file: entry.clone(),
                skipped: 0,
            }),
            PlayInput::Disc { entry: None, .. } => Err(PlayerError::SystemCannotPlayDisc),
        },
        PlayerKind::Mpv => Ok(LaunchPlan::Program {
            program: locate(PlayerKind::Mpv),
            args: mpv_args(input),
        }),
        PlayerKind::Vlc | PlayerKind::Iina => Ok(LaunchPlan::Program {
            program: locate(config.kind),
            args: plain_args(input),
        }),
        PlayerKind::MpcHc => Ok(LaunchPlan::Program {
            program: locate(PlayerKind::MpcHc),
            args: mpc_args(input),
        }),
        PlayerKind::Custom => custom_plan(&config.custom_command, input),
    }
}

/// Every file of the input, or the disc folder.
fn paths_of(input: &PlayInput) -> Vec<&Path> {
    match input {
        PlayInput::Files(files) => files.iter().map(PathBuf::as_path).collect(),
        PlayInput::Disc { root, .. } => vec![root.as_path()],
    }
}

fn plain_args(input: &PlayInput) -> Vec<OsString> {
    paths_of(input).into_iter().map(word).collect()
}

/// mpv queues every file in order. A DVD or Blu-ray folder is opened with the disc protocol and
/// the folder as the device; a disc image goes in as a plain file.
fn mpv_args(input: &PlayInput) -> Vec<OsString> {
    match input {
        PlayInput::Disc {
            root,
            kind: DiscKind::Dvd,
            ..
        } => vec![
            device_option("--dvd-device=", root),
            OsString::from("dvd://"),
        ],
        PlayInput::Disc {
            root,
            kind: DiscKind::BluRay,
            ..
        } => vec![
            device_option("--bluray-device=", root),
            OsString::from("bd://"),
        ],
        _ => plain_args(input),
    }
}

fn device_option(option: &str, path: &Path) -> OsString {
    let mut value = OsString::from(option);
    value.push(path);
    value
}

/// MPC-HC and MPC-BE take every file in order; a DVD folder needs `/dvd`.
fn mpc_args(input: &PlayInput) -> Vec<OsString> {
    let mut args = plain_args(input);
    if matches!(
        input,
        PlayInput::Disc {
            kind: DiscKind::Dvd,
            ..
        }
    ) {
        args.push(OsString::from("/dvd"));
    }
    args
}

/// Splits a custom command into words. Quotes group; they are not part of the word.
fn split_words(template: &str) -> Result<Vec<String>, PlayerError> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    for c in template.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(PlayerError::UnbalancedQuote);
    }
    if started {
        words.push(current);
    }
    Ok(words)
}

/// Checks a custom command without playing anything.
///
/// # Errors
/// Fails when the command is empty, a quote is not closed, or `{files}` is part of a longer word.
pub fn validate_custom_command(template: &str) -> Result<(), PlayerError> {
    let words = split_words(template)?;
    if words.is_empty() {
        return Err(PlayerError::EmptyCustomCommand);
    }
    if words
        .iter()
        .skip(1)
        .any(|w| w != "{files}" && w.contains("{files}"))
    {
        return Err(PlayerError::FilesPlaceholderNotAlone);
    }
    Ok(())
}

fn custom_plan(template: &str, input: &PlayInput) -> Result<LaunchPlan, PlayerError> {
    validate_custom_command(template)?;
    let words = split_words(template)?;
    let mut words = words.into_iter();
    let program = words.next().unwrap_or_default();
    let files = paths_of(input);
    let first = files.first().copied().unwrap_or(Path::new(""));
    let folder = match input {
        PlayInput::Disc { root, .. } => root.clone(),
        PlayInput::Files(_) => first.parent().map(Path::to_path_buf).unwrap_or_default(),
    };

    let rest: Vec<String> = words.collect();
    let uses_placeholder = rest
        .iter()
        .any(|w| w.contains("{file}") || w.contains("{files}") || w.contains("{folder}"));
    let mut args: Vec<OsString> = Vec::new();
    for w in &rest {
        if w == "{files}" {
            args.extend(files.iter().map(|p| word(p)));
        } else {
            args.push(fill(w, first, &folder));
        }
    }
    if !uses_placeholder {
        args.extend(files.iter().map(|p| word(p)));
    }
    Ok(LaunchPlan::Program {
        program: PathBuf::from(program),
        args,
    })
}

/// Replaces `{file}` and `{folder}` in one word.
fn fill(word: &str, file: &Path, folder: &Path) -> OsString {
    let mut out = OsString::new();
    let mut rest = word;
    while !rest.is_empty() {
        let next = [("{file}", file), ("{folder}", folder)]
            .into_iter()
            .filter_map(|(name, value)| rest.find(name).map(|at| (at, name, value)))
            .min_by_key(|(at, _, _)| *at);
        match next {
            Some((at, name, value)) => {
                out.push(&rest[..at]);
                out.push(value);
                rest = &rest[at + name.len()..];
            }
            None => {
                out.push(rest);
                break;
            }
        }
    }
    out
}

/// Carries a plan out. `open_system` opens a file with the operating system's default player.
///
/// A program is started detached: CineVault does not wait for the player to close.
///
/// # Errors
/// Fails when the program is not there or cannot start, or `open_system` fails.
pub fn launch(
    plan: &LaunchPlan,
    open_system: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<(), PlayerError> {
    match plan {
        LaunchPlan::System { file, .. } => open_system(file).map_err(PlayerError::System),
        LaunchPlan::Program { program, args } => spawn_detached(program, args),
    }
}

fn spawn_detached(program: &Path, args: &[OsString]) -> Result<(), PlayerError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: a console player (mpv.exe) must not flash a console window.
        command.creation_flags(0x0800_0000);
    }
    match command.spawn() {
        Ok(mut child) => {
            // Reap the player when it closes so it does not linger as a zombie process.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(PlayerError::NotFound(display_name(program)))
        }
        Err(error) => Err(PlayerError::Start(error.to_string())),
    }
}

fn display_name(program: &Path) -> String {
    match program.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => program.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> PlayInput {
        PlayInput::Files(names.iter().map(PathBuf::from).collect())
    }

    fn config(kind: PlayerKind) -> PlayerConfig {
        PlayerConfig {
            kind,
            ..PlayerConfig::default()
        }
    }

    fn nothing_installed(_: &Path) -> bool {
        false
    }

    fn plan_with(config: &PlayerConfig, input: &PlayInput) -> Result<LaunchPlan, PlayerError> {
        plan(config, input, Os::Windows, &[], &nothing_installed)
    }

    fn args_of(plan: LaunchPlan) -> Vec<String> {
        match plan {
            LaunchPlan::Program { args, .. } => args
                .into_iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            LaunchPlan::System { .. } => panic!("expected a program"),
        }
    }

    fn program_of(plan: &LaunchPlan) -> &Path {
        match plan {
            LaunchPlan::Program { program, .. } => program,
            LaunchPlan::System { .. } => panic!("expected a program"),
        }
    }

    fn dvd() -> PlayInput {
        PlayInput::Disc {
            root: "/movies/Heat".into(),
            kind: DiscKind::Dvd,
            entry: Some("/movies/Heat/VIDEO_TS/VIDEO_TS.IFO".into()),
        }
    }

    fn bluray() -> PlayInput {
        PlayInput::Disc {
            root: "/movies/Heat".into(),
            kind: DiscKind::BluRay,
            entry: None,
        }
    }

    #[test]
    fn kinds_round_trip_through_their_text() {
        for kind in PlayerKind::ALL {
            assert_eq!(PlayerKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(PlayerKind::parse("winamp"), None);
        assert_eq!(PlayerKind::default(), PlayerKind::System);
    }

    #[test]
    fn disc_folders_are_named_by_their_structure_folder() {
        assert_eq!(DiscKind::from_folder_name("VIDEO_TS"), Some(DiscKind::Dvd));
        assert_eq!(DiscKind::from_folder_name("bdmv"), Some(DiscKind::BluRay));
        assert_eq!(
            DiscKind::from_folder_name("HVDVD_TS"),
            Some(DiscKind::HdDvd)
        );
        assert_eq!(DiscKind::from_folder_name("extras"), None);
    }

    #[test]
    fn the_system_player_opens_the_first_part_and_reports_the_rest() {
        let input = files(&["/m/Heat cd1.mkv", "/m/Heat cd2.mkv"]);
        let plan = plan_with(&config(PlayerKind::System), &input).unwrap();
        assert_eq!(
            plan,
            LaunchPlan::System {
                file: "/m/Heat cd1.mkv".into(),
                skipped: 1
            }
        );
    }

    #[test]
    fn the_system_player_opens_the_index_file_of_a_disc() {
        let plan = plan_with(&config(PlayerKind::System), &dvd()).unwrap();
        assert_eq!(
            plan,
            LaunchPlan::System {
                file: "/movies/Heat/VIDEO_TS/VIDEO_TS.IFO".into(),
                skipped: 0
            }
        );
        assert_eq!(
            plan_with(&config(PlayerKind::System), &bluray()),
            Err(PlayerError::SystemCannotPlayDisc)
        );
    }

    #[test]
    fn mpv_queues_stacked_parts_in_order() {
        let input = files(&["/m/Heat cd1.mkv", "/m/Heat cd2.mkv"]);
        let plan = plan_with(&config(PlayerKind::Mpv), &input).unwrap();
        assert_eq!(program_of(&plan), Path::new("mpv"));
        assert_eq!(args_of(plan), ["/m/Heat cd1.mkv", "/m/Heat cd2.mkv"]);
    }

    #[test]
    fn mpv_plays_disc_folders_through_the_disc_protocols() {
        assert_eq!(
            args_of(plan_with(&config(PlayerKind::Mpv), &dvd()).unwrap()),
            ["--dvd-device=/movies/Heat", "dvd://"]
        );
        assert_eq!(
            args_of(plan_with(&config(PlayerKind::Mpv), &bluray()).unwrap()),
            ["--bluray-device=/movies/Heat", "bd://"]
        );
    }

    #[test]
    fn mpv_gets_a_disc_image_as_a_plain_file() {
        let input = files(&["/m/Heat.iso"]);
        assert_eq!(
            args_of(plan_with(&config(PlayerKind::Mpv), &input).unwrap()),
            ["/m/Heat.iso"]
        );
    }

    #[test]
    fn mpv_gets_an_hd_dvd_folder_as_a_plain_folder() {
        let input = PlayInput::Disc {
            root: "/movies/Heat".into(),
            kind: DiscKind::HdDvd,
            entry: None,
        };
        assert_eq!(
            args_of(plan_with(&config(PlayerKind::Mpv), &input).unwrap()),
            ["/movies/Heat"]
        );
    }

    #[test]
    fn vlc_and_iina_get_every_part_or_the_disc_folder() {
        let parts = files(&["/m/a.mkv", "/m/b.mkv", "/m/c.mkv"]);
        for kind in [PlayerKind::Vlc, PlayerKind::Iina] {
            assert_eq!(
                args_of(plan_with(&config(kind), &parts).unwrap()),
                ["/m/a.mkv", "/m/b.mkv", "/m/c.mkv"]
            );
            assert_eq!(
                args_of(plan_with(&config(kind), &dvd()).unwrap()),
                ["/movies/Heat"]
            );
        }
    }

    #[test]
    fn mpc_gets_every_part_and_marks_a_dvd_folder() {
        let parts = files(&["/m/a.mkv", "/m/b.mkv"]);
        assert_eq!(
            args_of(plan_with(&config(PlayerKind::MpcHc), &parts).unwrap()),
            ["/m/a.mkv", "/m/b.mkv"]
        );
        assert_eq!(
            args_of(plan_with(&config(PlayerKind::MpcHc), &dvd()).unwrap()),
            ["/movies/Heat", "/dvd"]
        );
        assert_eq!(
            args_of(plan_with(&config(PlayerKind::MpcHc), &bluray()).unwrap()),
            ["/movies/Heat"]
        );
    }

    #[test]
    fn a_name_that_starts_with_a_dash_is_not_an_option() {
        let input = files(&["-fullscreen.mkv"]);
        assert_eq!(
            args_of(plan_with(&config(PlayerKind::Mpv), &input).unwrap()),
            ["./-fullscreen.mkv"]
        );
    }

    #[test]
    fn a_configured_program_wins_over_everything() {
        let configured = PlayerConfig {
            kind: PlayerKind::Vlc,
            program: Some("D:\\Tools\\vlc.exe".into()),
            ..PlayerConfig::default()
        };
        let plan = plan(
            &configured,
            &files(&["a.mkv"]),
            Os::Windows,
            &[PathBuf::from("C:\\Program Files")],
            &|_| true,
        )
        .unwrap();
        assert_eq!(program_of(&plan), Path::new("D:\\Tools\\vlc.exe"));
    }

    #[test]
    fn an_installed_player_is_found_in_the_usual_place() {
        let dirs = [
            PathBuf::from("C:\\Program Files"),
            PathBuf::from("C:\\Program Files (x86)"),
        ];
        let x86_vlc = dirs[1].join("VideoLAN\\VLC\\vlc.exe");
        let found = resolve_program(PlayerKind::Vlc, None, Os::Windows, &dirs, &|p| p == x86_vlc);
        assert_eq!(found, x86_vlc);
        let none = resolve_program(PlayerKind::Vlc, None, Os::Windows, &dirs, &|_| false);
        assert_eq!(none, PathBuf::from("vlc"));
    }

    #[test]
    fn mpc_looks_for_mpc_be_after_mpc_hc() {
        let dirs = [PathBuf::from("C:\\Program Files")];
        let be = dirs[0].join("MPC-BE x64\\mpc-be64.exe");
        assert_eq!(
            resolve_program(PlayerKind::MpcHc, None, Os::Windows, &dirs, &|p| p == be),
            be
        );
        assert_eq!(
            resolve_program(PlayerKind::MpcHc, None, Os::Windows, &dirs, &|_| false),
            PathBuf::from("mpc-hc64")
        );
    }

    #[test]
    fn macos_players_are_found_in_their_application_folders() {
        let iina = PathBuf::from("/Applications/IINA.app/Contents/MacOS/iina-cli");
        assert_eq!(
            resolve_program(PlayerKind::Iina, None, Os::MacOs, &[], &|p| p == iina),
            iina
        );
        assert_eq!(
            resolve_program(PlayerKind::Iina, None, Os::MacOs, &[], &|_| false),
            PathBuf::from("iina-cli")
        );
        assert_eq!(
            resolve_program(PlayerKind::Mpv, None, Os::MacOs, &[], &|_| false),
            PathBuf::from("mpv")
        );
    }

    #[test]
    fn an_application_bundle_stands_for_the_program_inside() {
        let vlc = resolve_program(
            PlayerKind::Vlc,
            Some(Path::new("/Applications/VLC.app")),
            Os::MacOs,
            &[],
            &|_| false,
        );
        assert_eq!(vlc, Path::new("/Applications/VLC.app/Contents/MacOS/VLC"));
        let iina = resolve_program(
            PlayerKind::Iina,
            Some(Path::new("/Applications/IINA.app")),
            Os::MacOs,
            &[],
            &|_| false,
        );
        assert_eq!(
            iina,
            Path::new("/Applications/IINA.app/Contents/MacOS/iina-cli")
        );
    }

    fn custom(template: &str, input: &PlayInput) -> Result<LaunchPlan, PlayerError> {
        plan_with(
            &PlayerConfig {
                kind: PlayerKind::Custom,
                program: None,
                custom_command: template.to_owned(),
            },
            input,
        )
    }

    #[test]
    fn a_custom_command_without_placeholders_gets_the_files_at_the_end() {
        let input = files(&["/m/a.mkv", "/m/b.mkv"]);
        let plan = custom("player --fullscreen", &input).unwrap();
        assert_eq!(program_of(&plan), Path::new("player"));
        assert_eq!(args_of(plan), ["--fullscreen", "/m/a.mkv", "/m/b.mkv"]);
    }

    #[test]
    fn custom_placeholders_are_replaced() {
        let input = files(&["/m/Heat/a.mkv", "/m/Heat/b.mkv"]);
        let plan = custom(
            r#""C:\Program Files\Player\p.exe" --title={file} {files} --dir "{folder}""#,
            &input,
        )
        .unwrap();
        assert_eq!(
            program_of(&plan),
            Path::new(r"C:\Program Files\Player\p.exe")
        );
        assert_eq!(
            args_of(plan),
            [
                "--title=/m/Heat/a.mkv",
                "/m/Heat/a.mkv",
                "/m/Heat/b.mkv",
                "--dir",
                "/m/Heat"
            ]
        );
    }

    #[test]
    fn custom_placeholders_name_the_disc_folder_for_a_disc() {
        let plan = custom("p --open {file} --in {folder}", &dvd()).unwrap();
        assert_eq!(
            args_of(plan),
            ["--open", "/movies/Heat", "--in", "/movies/Heat"]
        );
    }

    #[test]
    fn a_file_name_with_spaces_or_quotes_stays_one_argument() {
        let input = files(&["/m/Heat \"Director's Cut\" cd1.mkv"]);
        let plan = custom("p {file}", &input).unwrap();
        assert_eq!(args_of(plan), ["/m/Heat \"Director's Cut\" cd1.mkv"]);
    }

    #[test]
    fn a_file_name_cannot_inject_arguments() {
        let input = files(&["/m/a.mkv --script=evil.lua"]);
        let plan = custom("p {files}", &input).unwrap();
        assert_eq!(args_of(plan), ["/m/a.mkv --script=evil.lua"]);
    }

    #[test]
    fn quotes_group_words_and_may_make_an_empty_argument() {
        assert_eq!(
            split_words(r#"a 'b c' "d e" "" f"#).unwrap(),
            ["a", "b c", "d e", "", "f"]
        );
    }

    #[test]
    fn a_bad_custom_command_is_refused_before_anything_runs() {
        let input = files(&["a.mkv"]);
        assert_eq!(custom("   ", &input), Err(PlayerError::EmptyCustomCommand));
        assert_eq!(
            custom("p \"open", &input),
            Err(PlayerError::UnbalancedQuote)
        );
        assert_eq!(
            custom("p --list={files}", &input),
            Err(PlayerError::FilesPlaceholderNotAlone)
        );
        assert_eq!(validate_custom_command("p {files}"), Ok(()));
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_is_not_there_is_reported_by_name() {
        let plan = LaunchPlan::Program {
            program: "/definitely/not/here/cv-player".into(),
            args: Vec::new(),
        };
        let error = launch(&plan, &|_| Ok(())).unwrap_err();
        assert_eq!(error, PlayerError::NotFound("cv-player".into()));
        assert_eq!(error.code(), "player_not_found");
    }

    #[cfg(windows)]
    #[test]
    fn a_program_that_is_not_there_is_reported_by_name() {
        let plan = LaunchPlan::Program {
            program: "C:\\definitely\\not\\here\\cv-player.exe".into(),
            args: Vec::new(),
        };
        let error = launch(&plan, &|_| Ok(())).unwrap_err();
        assert_eq!(error, PlayerError::NotFound("cv-player.exe".into()));
    }

    #[test]
    fn the_system_player_is_opened_through_the_callback() {
        use std::cell::RefCell;
        let opened = RefCell::new(Vec::new());
        let plan = LaunchPlan::System {
            file: "/m/a.mkv".into(),
            skipped: 0,
        };
        launch(&plan, &|p| {
            opened.borrow_mut().push(p.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(*opened.borrow(), [PathBuf::from("/m/a.mkv")]);
        let failing = launch(&plan, &|_| Err("no handler".into())).unwrap_err();
        assert_eq!(failing, PlayerError::System("no handler".into()));
    }
}
