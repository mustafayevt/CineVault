//! External tools for the app: the tools manager plus the user's chosen paths, which are kept in
//! the settings file so they survive a restart.
//!
//! The GUI and the CLI share this service, so they locate and install tools the
//! same way. A path set here wins over a download (see `cv_media::tools`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use cv_media::tools::{
    Cancel, InstallProgress, PlatformSpec, ToolError, ToolId, ToolStatus, ToolsManager,
};
use serde_json::Value;

use crate::app_settings::{AppSettingsError, SettingsService};

/// The settings key that holds the user's path for `tool`, for example `tools.path.ffmpeg`.
#[must_use]
pub fn path_setting_key(tool: ToolId) -> String {
    format!("tools.path.{}", tool.as_str())
}

/// Why a tools request failed.
#[derive(Debug, thiserror::Error)]
pub enum ToolsError {
    #[error("{0}")]
    Tool(#[from] ToolError),
    #[error("CineVault could not save the setting: {0}")]
    Settings(#[from] AppSettingsError),
    #[error("{0} is not a tool CineVault knows.")]
    UnknownTool(String),
    #[error("Another download is already running.")]
    Busy,
}

impl ToolsError {
    /// A short stable name for the kind of failure.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Tool(error) => error.code(),
            Self::Settings(_) => "storage",
            Self::UnknownTool(_) => "unknown_tool",
            Self::Busy => "busy",
        }
    }
}

/// Finds, downloads and reports on the external tools.
#[derive(Debug)]
pub struct ToolsService {
    manager: Arc<ToolsManager>,
    current: Mutex<Option<Cancel>>,
}

impl ToolsService {
    /// A service keeping downloads under `tools_dir` and restoring the paths stored in
    /// `settings`.
    #[must_use]
    pub fn open(tools_dir: impl Into<PathBuf>, settings: &SettingsService) -> Self {
        Self::with_manager(ToolsManager::new(tools_dir), settings)
    }

    /// Like [`Self::open`] with a prepared manager (tests).
    #[must_use]
    pub fn with_manager(manager: ToolsManager, settings: &SettingsService) -> Self {
        for tool in ToolId::LISTED {
            if let Some(Value::String(path)) = settings.get(&path_setting_key(tool))
                && !path.trim().is_empty()
            {
                // Not checked here: a drive that is not mounted yet must not lose the setting.
                manager.restore_override(tool, path);
            }
        }
        Self {
            manager: Arc::new(manager),
            current: Mutex::new(None),
        }
    }

    /// The manager, for code that needs a tool (`resolve`).
    #[must_use]
    pub fn manager(&self) -> Arc<ToolsManager> {
        Arc::clone(&self.manager)
    }

    /// The tool with the given id.
    ///
    /// # Errors
    /// [`ToolsError::UnknownTool`] for an id that is not a tool.
    pub fn parse_tool(id: &str) -> Result<ToolId, ToolsError> {
        ToolId::LISTED
            .into_iter()
            .find(|tool| tool.as_str() == id)
            .ok_or_else(|| ToolsError::UnknownTool(id.to_owned()))
    }

    /// The state of every tool. Starts the user's programs briefly to read their versions, so
    /// call it off the UI thread.
    #[must_use]
    pub fn status(&self) -> Vec<ToolStatus> {
        self.manager.status()
    }

    /// Sets, or with `None` clears, the path for `tool` and saves it. A folder is accepted. A
    /// program is started once with its version flag to check that it is the tool.
    ///
    /// # Errors
    /// The path is checked first ([`ToolError::OverrideInvalid`]: not the tool's file name, or a
    /// program that does not answer like the tool); when saving fails the change is undone.
    pub fn set_path(
        &self,
        settings: &SettingsService,
        tool: ToolId,
        path: Option<&Path>,
    ) -> Result<(), ToolsError> {
        let previous = self.manager.override_for(tool);
        // The user's choice is checked to really be the tool before anything is saved.
        let checked = path
            .map(|path| self.manager.check_override(tool, path))
            .transpose()?;
        let chosen = self.manager.set_override(tool, checked.as_deref())?;
        let key = path_setting_key(tool);
        let saved = match chosen {
            Some(file) => settings.set(&key, Value::String(file.to_string_lossy().into_owned())),
            None => settings.reset(&[key]),
        };
        if let Err(error) = saved {
            // Keep the running app and the settings file in step.
            match previous {
                Some(old) => self.manager.restore_override(tool, old),
                None => {
                    let _ = self.manager.set_override(tool, None);
                }
            }
            return Err(error.into());
        }
        Ok(())
    }

    /// Downloads the package that provides `tool`, reporting progress. Only one download runs at
    /// a time.
    ///
    /// # Errors
    /// [`ToolsError::Busy`] if another download is running, otherwise the install error. A
    /// corrupted download fails with a checksum error and installs nothing.
    pub fn install(
        &self,
        tool: ToolId,
        progress: &mut dyn FnMut(InstallProgress),
    ) -> Result<(), ToolsError> {
        let cancel = Cancel::new();
        {
            let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
            if current.is_some() {
                return Err(ToolsError::Busy);
            }
            *current = Some(cancel.clone());
        }
        let result = self.manager.install(tool.package(), &cancel, progress);
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = None;
        result.map_err(ToolsError::from)
    }

    /// Like [`Self::install`] for a download that is not the one this build pins (a newer
    /// indexer service whose published digest was read from its release). The same busy rule,
    /// staging and checksum apply.
    ///
    /// # Errors
    /// [`ToolsError::Busy`] if another download is running, otherwise the install error.
    pub fn install_spec(
        &self,
        tool: ToolId,
        spec: &PlatformSpec,
        progress: &mut dyn FnMut(InstallProgress),
    ) -> Result<(), ToolsError> {
        let cancel = Cancel::new();
        {
            let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
            if current.is_some() {
                return Err(ToolsError::Busy);
            }
            *current = Some(cancel.clone());
        }
        let result = self
            .manager
            .install_spec(tool.package(), spec, &cancel, progress);
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = None;
        result.map_err(ToolsError::from)
    }

    /// Announces that the tools `ids` are about to be downloaded, one after the other, so work
    /// that needs them waits for them instead of reporting them missing.
    /// Unknown ids are ignored. A tool that is ready already is not waited for.
    pub fn expect(&self, ids: &[String]) {
        for id in ids {
            let Ok(tool) = Self::parse_tool(id) else {
                continue;
            };
            if self.manager.resolve(tool).is_err() {
                self.manager.expect(tool.package());
            }
        }
    }

    /// Stops the running download. Returns false when none is running.
    pub fn cancel_install(&self) -> bool {
        match &*self.current.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingsStore;

    fn settings(dir: &Path) -> SettingsService {
        SettingsService::open(SettingsStore::new(dir.join("settings.json")))
    }

    /// A stand-in FFmpeg that answers its version flag, as setting a path checks.
    fn program(dir: &Path, name: &str) -> PathBuf {
        #[cfg(windows)]
        let path = {
            let path = dir.join(format!("{name}.bat"));
            std::fs::write(
                &path,
                "@echo off
echo ffmpeg version 1.0-test
",
            )
            .expect("write");
            path
        };
        #[cfg(unix)]
        let path = {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join(name);
            std::fs::write(
                &path,
                "#!/bin/sh
echo 'ffmpeg version 1.0-test'
",
            )
            .expect("write");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
            path
        };
        path
    }

    #[test]
    fn a_chosen_path_survives_a_restart() {
        let data = tempfile::tempdir().expect("temp dir");
        let mine = tempfile::tempdir().expect("temp dir");
        let ffmpeg = program(mine.path(), "my-ffmpeg");

        let first_settings = settings(data.path());
        let first = ToolsService::open(data.path().join("tools"), &first_settings);
        first
            .set_path(&first_settings, ToolId::Ffmpeg, Some(&ffmpeg))
            .expect("set");
        assert_eq!(
            first_settings.get("tools.path.ffmpeg"),
            Some(Value::String(ffmpeg.to_string_lossy().into_owned()))
        );

        // A new process reads the same settings file.
        let second_settings = settings(data.path());
        let second = ToolsService::open(data.path().join("tools"), &second_settings);
        let resolved = second.manager().resolve(ToolId::Ffmpeg).expect("resolve");
        assert_eq!(resolved.path, ffmpeg);
    }

    #[test]
    fn clearing_a_path_removes_the_setting() {
        let data = tempfile::tempdir().expect("temp dir");
        let mine = tempfile::tempdir().expect("temp dir");
        let ffmpeg = program(mine.path(), "my-ffmpeg");
        let settings = settings(data.path());
        let service = ToolsService::open(data.path().join("tools"), &settings);
        service
            .set_path(&settings, ToolId::Ffmpeg, Some(&ffmpeg))
            .expect("set");
        service
            .set_path(&settings, ToolId::Ffmpeg, None)
            .expect("clear");
        assert_eq!(settings.get("tools.path.ffmpeg"), None);
        assert_eq!(service.manager().override_for(ToolId::Ffmpeg), None);
    }

    #[test]
    fn a_bad_path_is_refused_and_not_saved() {
        let data = tempfile::tempdir().expect("temp dir");
        let settings = settings(data.path());
        let service = ToolsService::open(data.path().join("tools"), &settings);
        let error = service
            .set_path(
                &settings,
                ToolId::Ffmpeg,
                Some(&data.path().join("missing")),
            )
            .expect_err("missing file");
        assert_eq!(error.code(), "override_invalid");
        assert_eq!(settings.get("tools.path.ffmpeg"), None);
    }

    #[test]
    fn another_program_is_refused_as_the_tool_and_not_saved() {
        let data = tempfile::tempdir().expect("temp dir");
        let mine = tempfile::tempdir().expect("temp dir");
        let settings = settings(data.path());
        let service = ToolsService::open(data.path().join("tools"), &settings);
        // A program that is not FFmpeg, even one that answers like FFmpeg.
        let other = program(mine.path(), "notepad");
        let error = service
            .set_path(&settings, ToolId::Ffmpeg, Some(&other))
            .expect_err("not ffmpeg");
        assert_eq!(error.code(), "override_invalid");
        assert!(error.to_string().contains("not ffmpeg"), "{error}");
        // Named like FFmpeg but answering like something else.
        let renamed = odd_program(mine.path(), "ffmpeg");
        let error = service
            .set_path(&settings, ToolId::Ffmpeg, Some(&renamed))
            .expect_err("does not answer like ffmpeg");
        assert!(error.to_string().contains("did not answer"), "{error}");
        assert_eq!(settings.get("tools.path.ffmpeg"), None);
        assert_eq!(service.manager().override_for(ToolId::Ffmpeg), None);
    }

    #[test]
    fn a_stored_path_to_a_missing_file_is_kept_and_reported() {
        let data = tempfile::tempdir().expect("temp dir");
        let settings = settings(data.path());
        let gone = data.path().join("unmounted").join("ffmpeg");
        settings
            .set(
                "tools.path.ffmpeg",
                Value::String(gone.to_string_lossy().into_owned()),
            )
            .expect("set");
        let service = ToolsService::open(data.path().join("tools"), &settings);
        let status = service.status();
        let ffmpeg = status.iter().find(|s| s.id == ToolId::Ffmpeg).expect("row");
        assert_eq!(ffmpeg.override_path.as_deref(), Some(gone.as_path()));
        assert_eq!(
            service
                .manager()
                .resolve(ToolId::Ffmpeg)
                .unwrap_err()
                .code(),
            "override_missing"
        );
    }

    /// A program called `name` that answers its version flag like some other program.
    fn odd_program(dir: &Path, name: &str) -> PathBuf {
        let banner = "something-else v103.0";
        #[cfg(windows)]
        let path = {
            let path = dir.join(format!("{name}.bat"));
            std::fs::write(
                &path,
                format!(
                    "@echo off
echo {banner}
"
                ),
            )
            .expect("write");
            path
        };
        #[cfg(unix)]
        let path = {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join(name);
            std::fs::write(
                &path,
                format!(
                    "#!/bin/sh
echo '{banner}'
"
                ),
            )
            .expect("write");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
            path
        };
        path
    }

    #[test]
    fn unknown_tools_and_idle_cancel_are_handled() {
        assert_eq!(
            ToolsService::parse_tool("nope").unwrap_err().code(),
            "unknown_tool"
        );
        assert_eq!(
            ToolsService::parse_tool("ffmpeg").expect("known"),
            ToolId::Ffmpeg
        );
        assert_eq!(
            ToolsService::parse_tool("yt-dlp").unwrap_err().code(),
            "unknown_tool",
            "tools that were removed are not known"
        );
        let data = tempfile::tempdir().expect("temp dir");
        let settings = settings(data.path());
        let service = ToolsService::open(data.path().join("tools"), &settings);
        assert!(!service.cancel_install());
    }

    #[test]
    fn announced_downloads_of_missing_tools_are_coming() {
        let data = tempfile::tempdir().expect("temp dir");
        let settings = settings(data.path());
        let service = ToolsService::open(data.path().join("tools"), &settings);
        service.expect(&["mediainfo".to_owned(), "nope".to_owned()]);
        let manager = service.manager();
        assert!(manager.is_coming(cv_media::tools::Package::MediaInfo));
        assert!(!manager.is_coming(cv_media::tools::Package::Ffmpeg));
    }
}
