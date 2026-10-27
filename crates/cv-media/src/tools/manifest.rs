//! The pinned manifest: which tool versions CineVault downloads, from where, and the SHA-256
//! each download must have.
//!
//! The manifest is JSON embedded in the binary (`manifest.json`), so a release always knows the
//! exact bytes it accepts. Updating a tool means changing a URL and a hash here and nothing else.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

use super::ToolError;

/// The manifest shipped with this build.
const EMBEDDED: &str = include_str!("manifest.json");

/// The manifest schema this code understands.
const SCHEMA: u32 = 1;

/// A tool CineVault can locate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolId {
    /// The MediaInfo library (`MediaInfo.dll`, `libmediainfo.dylib`), loaded by the app.
    MediaInfo,
    Ffmpeg,
    /// The local program that reaches the sites that need an indexer (Jackett). It has its own
    /// card in Settings, not a row among the tools.
    IndexerService,
}

impl ToolId {
    /// Every tool.
    pub const ALL: [Self; 3] = [Self::MediaInfo, Self::Ffmpeg, Self::IndexerService];

    /// The tools the Tools page lists as rows, in order. A person may point these at their own
    /// copy; the indexer service is not among them.
    pub const LISTED: [Self; 2] = [Self::MediaInfo, Self::Ffmpeg];

    /// The stable id used in the manifest, settings keys and commands.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MediaInfo => "mediainfo",
            Self::Ffmpeg => "ffmpeg",
            Self::IndexerService => "indexer-service",
        }
    }

    /// The tool with the given id.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.as_str() == id)
    }

    /// The download that provides this tool.
    #[must_use]
    pub const fn package(self) -> Package {
        match self {
            Self::MediaInfo => Package::MediaInfo,
            Self::Ffmpeg => Package::Ffmpeg,
            Self::IndexerService => Package::IndexerService,
        }
    }

    /// Whether the tool is a library the app loads rather than a program it starts.
    #[must_use]
    pub const fn is_library(self) -> bool {
        matches!(self, Self::MediaInfo)
    }

    /// The name every copy of the tool has in its file name, whatever the build adds around it
    /// (`ffmpeg.exe`, `libmediainfo.0.dylib`). A file the user chooses must contain it, so an
    /// unrelated program is never started to ask its version.
    #[must_use]
    pub const fn program_name(self) -> &'static str {
        match self {
            Self::MediaInfo => "mediainfo",
            Self::Ffmpeg => "ffmpeg",
            Self::IndexerService => "jackett",
        }
    }

    /// The file name of the tool on this OS, for a folder the user points at.
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        if cfg!(windows) {
            match self {
                Self::MediaInfo => "MediaInfo.dll",
                Self::Ffmpeg => "ffmpeg.exe",
                Self::IndexerService => "JackettConsole.exe",
            }
        } else {
            match self {
                Self::MediaInfo => "libmediainfo.dylib",
                Self::Ffmpeg => "ffmpeg",
                Self::IndexerService => "jackett",
            }
        }
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One download. It installs into `<tools>/<package>/<version>/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Package {
    MediaInfo,
    Ffmpeg,
    IndexerService,
}

impl Package {
    pub const ALL: [Self; 3] = [Self::MediaInfo, Self::Ffmpeg, Self::IndexerService];

    /// The packages the Tools page lists (the ones with a row), which a development fetch
    /// downloads when no package is named.
    pub const LISTED: [Self; 2] = [Self::MediaInfo, Self::Ffmpeg];

    /// The key in the manifest and the folder name under the tools folder.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MediaInfo => "mediainfo",
            Self::Ffmpeg => "ffmpeg",
            Self::IndexerService => "indexer-service",
        }
    }

    /// The tools this package provides.
    #[must_use]
    pub fn tools(self) -> Vec<ToolId> {
        ToolId::ALL
            .into_iter()
            .filter(|tool| tool.package() == self)
            .collect()
    }
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An OS and CPU the manifest has downloads for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Platform {
    WindowsX64,
    MacosX64,
    MacosArm64,
}

impl Platform {
    pub const ALL: [Self; 3] = [Self::WindowsX64, Self::MacosX64, Self::MacosArm64];

    /// The platform this build runs on, or `None` where no downloads exist.
    #[must_use]
    pub const fn current() -> Option<Self> {
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Some(Self::WindowsX64)
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Some(Self::MacosX64)
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Some(Self::MacosArm64)
        } else {
            None
        }
    }

    /// The key in the manifest.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::WindowsX64 => "windows-x86_64",
            Self::MacosX64 => "macos-x86_64",
            Self::MacosArm64 => "macos-aarch64",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// How a download is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ArchiveFormat {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.bz2")]
    TarBz2,
    #[serde(rename = "tar.gz")]
    TarGz,
    /// The download is the program itself.
    #[serde(rename = "raw")]
    Raw,
}

/// A file taken out of a download.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FileSpec {
    /// The tool this file is, or `None` for a file kept alongside it (a licence text).
    #[serde(default)]
    pub provides: Option<String>,
    /// The path of the file inside the archive, with `/` separators. Ignored for raw downloads.
    pub from: String,
    /// The file name it gets in the install folder.
    pub to: String,
}

impl FileSpec {
    /// The tool this file is.
    #[must_use]
    pub fn tool(&self) -> Option<ToolId> {
        self.provides.as_deref().and_then(ToolId::parse)
    }
}

/// What is taken out of a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum ArtifactMode {
    /// The files listed in [`Artifact::files`], each under the name given for it.
    #[default]
    #[serde(rename = "files")]
    Files,
    /// Everything below [`Artifact::root`], with the same relative paths, for a program that comes
    /// with many files of its own.
    #[serde(rename = "tree")]
    Tree,
}

/// The program of a tree install: the file that is the tool.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProgramSpec {
    /// The tool this file is.
    pub provides: String,
    /// Its path below the root of the tree, with `/` separators.
    pub path: String,
}

/// The most a tree install may unpack to, in bytes.
pub const TREE_MAX_BYTES: u64 = 400 * 1024 * 1024;

/// One file to download, with the hash it must have.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Artifact {
    pub url: String,
    /// Lower-case hex SHA-256 of the downloaded file.
    pub sha256: String,
    /// The exact size in bytes. A download that grows past it is abandoned.
    pub size: u64,
    pub format: ArchiveFormat,
    /// What to take from the archive. Empty in a tree install.
    #[serde(default)]
    pub files: Vec<FileSpec>,
    #[serde(default)]
    pub mode: ArtifactMode,
    /// A tree install: the folder inside the archive whose content is installed (`Jackett/`).
    /// Anything outside it is refused.
    #[serde(default)]
    pub root: Option<String>,
    /// A tree install: the file that is the tool.
    #[serde(default)]
    pub program: Option<ProgramSpec>,
    /// A tree install: a lower limit than [`TREE_MAX_BYTES`] for the unpacked size.
    #[serde(default)]
    pub max_unpacked: Option<u64>,
}

impl Artifact {
    /// The most this artifact may unpack to.
    #[must_use]
    pub fn unpack_limit(&self) -> u64 {
        self.max_unpacked
            .map_or(TREE_MAX_BYTES, |limit| limit.min(TREE_MAX_BYTES))
    }

    /// The folder prefix of a tree install, always ending in `/`.
    #[must_use]
    pub fn tree_root(&self) -> String {
        let root = self.root.as_deref().unwrap_or_default().replace('\\', "/");
        let root = root.trim_start_matches("./").trim_matches('/');
        if root.is_empty() {
            String::new()
        } else {
            format!("{root}/")
        }
    }
}

/// What to download for one package on one platform.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PlatformSpec {
    pub version: String,
    pub artifacts: Vec<Artifact>,
}

impl PlatformSpec {
    /// Checks the spec by the rules every manifest entry follows: a version, at least one artifact,
    /// valid artifacts, and a file for every tool of `package`.
    ///
    /// # Errors
    /// [`ToolError::Manifest`] with the first rule that is broken.
    pub fn validate(&self, package: Package) -> Result<(), ToolError> {
        self.validate_in(package.as_str(), package)
    }

    fn validate_in(&self, context: &str, package: Package) -> Result<(), ToolError> {
        let bad = |message: String| Err(ToolError::Manifest(message));
        if self.version.trim().is_empty() || self.artifacts.is_empty() {
            return bad(format!("{context}: needs a version and an artifact"));
        }
        for artifact in &self.artifacts {
            validate_artifact(context, artifact)?;
        }
        let provided = self.tool_files();
        for tool in package.tools() {
            if !provided.contains_key(&tool) {
                return bad(format!("{context}: nothing provides {tool}"));
            }
        }
        Ok(())
    }

    /// The total download size in bytes.
    #[must_use]
    pub fn download_size(&self) -> u64 {
        self.artifacts.iter().map(|artifact| artifact.size).sum()
    }

    /// The file names the install must contain, by tool.
    #[must_use]
    pub fn tool_files(&self) -> BTreeMap<ToolId, String> {
        let listed = self
            .artifacts
            .iter()
            .flat_map(|artifact| &artifact.files)
            .filter_map(|file| file.tool().map(|tool| (tool, file.to.clone())));
        let programs = self
            .artifacts
            .iter()
            .filter_map(|artifact| artifact.program.as_ref())
            .filter_map(|program| {
                ToolId::parse(&program.provides).map(|tool| (tool, program.path.clone()))
            });
        listed.chain(programs).collect()
    }
}

/// A package in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PackageSpec {
    /// Name shown to people.
    pub display: String,
    /// The licence of the downloaded program, as text.
    pub license: String,
    pub homepage: String,
    pub platforms: BTreeMap<String, PlatformSpec>,
}

/// The pinned tool downloads.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    schema: u32,
    packages: BTreeMap<String, PackageSpec>,
}

impl Manifest {
    /// The manifest of this build.
    ///
    /// # Panics
    /// Never in a released build: a test parses and validates the embedded file.
    #[must_use]
    pub fn embedded() -> Self {
        Self::parse(EMBEDDED).expect("the embedded tool manifest is valid")
    }

    /// Parses and validates manifest JSON.
    ///
    /// # Errors
    /// Fails when the JSON is malformed or breaks a manifest rule (see [`Self::validate`]).
    pub fn parse(json: &str) -> Result<Self, ToolError> {
        let manifest: Self = serde_json::from_str(json)
            .map_err(|error| ToolError::Manifest(format!("not valid JSON: {error}")))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// The rules every manifest follows:
    /// * the schema is the one this code reads;
    /// * every package is a known one and has at least one artifact on each platform it lists;
    /// * every hash is 64 lower-case hex digits and every size is above zero;
    /// * every file a tool needs is provided, and install names are plain file names.
    ///
    /// The `https` rule is enforced when downloading, so tests can serve fixtures from loopback.
    ///
    /// # Errors
    /// Fails with the first rule that is broken.
    pub fn validate(&self) -> Result<(), ToolError> {
        let bad = |message: String| Err(ToolError::Manifest(message));
        if self.schema != SCHEMA {
            return bad(format!("schema {} is not supported", self.schema));
        }
        for (key, package) in &self.packages {
            let Some(id) = Package::ALL.into_iter().find(|p| p.as_str() == key) else {
                return bad(format!("unknown package {key:?}"));
            };
            for (platform_key, spec) in &package.platforms {
                if !Platform::ALL.iter().any(|p| p.key() == platform_key) {
                    return bad(format!("{key}: unknown platform {platform_key:?}"));
                }
                spec.validate_in(&format!("{key} on {platform_key}"), id)?;
            }
        }
        Ok(())
    }

    /// The package's entry.
    #[must_use]
    pub fn package(&self, package: Package) -> Option<&PackageSpec> {
        self.packages.get(package.as_str())
    }

    /// What to download for `package` on `platform`.
    #[must_use]
    pub fn platform_spec(&self, package: Package, platform: Platform) -> Option<&PlatformSpec> {
        self.package(package)?.platforms.get(platform.key())
    }
}

fn validate_artifact(context: &str, artifact: &Artifact) -> Result<(), ToolError> {
    let bad = |message: String| Err(ToolError::Manifest(format!("{context}: {message}")));
    if !is_sha256_hex(&artifact.sha256) {
        return bad(format!("{:?} is not a SHA-256", artifact.sha256));
    }
    if artifact.size == 0 {
        return bad("a size is required".to_owned());
    }
    if artifact.mode == ArtifactMode::Tree {
        return validate_tree(&bad, artifact);
    }
    if artifact.root.is_some() || artifact.program.is_some() || artifact.max_unpacked.is_some() {
        return bad(format!(
            "{} sets tree options on a download of listed files",
            artifact.url
        ));
    }
    if artifact.files.is_empty() {
        return bad(format!("{} lists no files to install", artifact.url));
    }
    if artifact.format == ArchiveFormat::Raw && artifact.files.len() != 1 {
        return bad(format!("{} is a raw download of one file", artifact.url));
    }
    for file in &artifact.files {
        if !is_plain_file_name(&file.to) {
            return bad(format!("{:?} is not a plain file name", file.to));
        }
        if file.from.is_empty() || file.from.starts_with('/') || file.from.contains("..") {
            return bad(format!("{:?} is not a safe archive path", file.from));
        }
        if let Some(provides) = &file.provides
            && ToolId::parse(provides).is_none()
        {
            return bad(format!("unknown tool {provides:?}"));
        }
    }
    Ok(())
}

/// The rules of a tree install: an archive format, a root, a program below it, no listed files.
fn validate_tree(
    bad: &dyn Fn(String) -> Result<(), ToolError>,
    artifact: &Artifact,
) -> Result<(), ToolError> {
    if artifact.format == ArchiveFormat::Raw {
        return bad(format!(
            "{} cannot be a raw download as a tree",
            artifact.url
        ));
    }
    if !artifact.files.is_empty() {
        return bad(format!("{} is a tree and lists no files", artifact.url));
    }
    let root = artifact.tree_root();
    if root.contains("..") || root.contains(':') || root.contains('\0') {
        return bad(format!("{:?} is not a safe root folder", artifact.root));
    }
    let Some(program) = &artifact.program else {
        return bad(format!("{} names no program", artifact.url));
    };
    if ToolId::parse(&program.provides).is_none() {
        return bad(format!("unknown tool {:?}", program.provides));
    }
    if !is_safe_relative(&program.path) {
        return bad(format!("{:?} is not a safe program path", program.path));
    }
    if artifact.max_unpacked == Some(0) {
        return bad("the unpacked limit must be above zero".to_owned());
    }
    Ok(())
}

/// Whether `path` is a relative path of plain names with `/` separators.
fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.contains(['\\', ':', '\0', '*', '?', '"', '<', '>', '|'])
        })
}

/// Whether `text` is 64 lower-case hex digits.
#[must_use]
pub fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && name != "install.json"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_manifest_is_valid_and_covers_every_platform() {
        let manifest = Manifest::embedded();
        for package in Package::ALL {
            for platform in Platform::ALL {
                let spec = manifest
                    .platform_spec(package, platform)
                    .unwrap_or_else(|| panic!("{package} has no entry for {platform}"));
                for tool in package.tools() {
                    assert!(
                        spec.tool_files().contains_key(&tool),
                        "{tool} on {platform}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_embedded_download_is_pinned_over_https() {
        let manifest = Manifest::embedded();
        for package in Package::ALL {
            for platform in Platform::ALL {
                let spec = manifest.platform_spec(package, platform).expect("entry");
                for artifact in &spec.artifacts {
                    assert!(artifact.url.starts_with("https://"), "{}", artifact.url);
                    assert!(is_sha256_hex(&artifact.sha256), "{}", artifact.url);
                    assert!(artifact.size > 0);
                }
            }
        }
    }

    #[test]
    fn tool_ids_round_trip_through_text() {
        for tool in ToolId::ALL {
            assert_eq!(ToolId::parse(tool.as_str()), Some(tool));
        }
        assert_eq!(ToolId::parse("MediaInfo"), None);
    }

    #[test]
    fn the_indexer_service_is_pinned_as_a_tree_on_every_platform() {
        let manifest = Manifest::embedded();
        let expected = [
            (
                Platform::WindowsX64,
                "Jackett.Binaries.Windows.zip",
                57_061_988,
                "c4ea2254688017f6e4ef0b9f5abc128e9134793b1995bc8d527fb8b6e6a3a25e",
                ArchiveFormat::Zip,
                "JackettConsole.exe",
            ),
            (
                Platform::MacosX64,
                "Jackett.Binaries.macOS.tar.gz",
                50_127_041,
                "e78b21e76df9a8329cddd4a55da9ef5353486f2649523040803c0021a1310a8d",
                ArchiveFormat::TarGz,
                "jackett",
            ),
            (
                Platform::MacosArm64,
                "Jackett.Binaries.macOSARM64.tar.gz",
                47_876_613,
                "c329944b8e539b2d4e63cb200b2cf75ff3e6e0e6cdd6f71140a0a088d41e5240",
                ArchiveFormat::TarGz,
                "jackett",
            ),
        ];
        for (platform, asset, size, sha, format, program) in expected {
            let spec = manifest
                .platform_spec(Package::IndexerService, platform)
                .unwrap_or_else(|| panic!("no entry for {platform}"));
            assert_eq!(spec.version, "0.24.2903");
            let [artifact] = spec.artifacts.as_slice() else {
                panic!("one artifact on {platform}");
            };
            assert_eq!(
                artifact.url,
                format!("https://github.com/Jackett/Jackett/releases/download/v0.24.2903/{asset}")
            );
            assert_eq!((artifact.size, artifact.sha256.as_str()), (size, sha));
            assert_eq!(artifact.format, format);
            assert_eq!(artifact.mode, ArtifactMode::Tree);
            assert_eq!(artifact.tree_root(), "Jackett/");
            assert_eq!(
                spec.tool_files()
                    .get(&ToolId::IndexerService)
                    .map(String::as_str),
                Some(program)
            );
        }
        let package = manifest.package(Package::IndexerService).expect("package");
        assert_eq!(package.display, "Indexer service (Jackett)");
        assert_eq!(package.license, "GPL-2.0");
    }

    #[test]
    fn the_indexer_service_is_a_tool_but_not_a_row() {
        assert!(ToolId::ALL.contains(&ToolId::IndexerService));
        assert!(!ToolId::LISTED.contains(&ToolId::IndexerService));
        assert!(!Package::LISTED.contains(&Package::IndexerService));
        assert_eq!(
            ToolId::parse("indexer-service"),
            Some(ToolId::IndexerService)
        );
        assert_eq!(ToolId::IndexerService.package(), Package::IndexerService);
        assert_eq!(Package::IndexerService.tools(), [ToolId::IndexerService]);
    }

    fn with(replace: &str, by: &str) -> String {
        EMBEDDED.replacen(replace, by, 1)
    }

    #[test]
    fn a_manifest_with_a_bad_hash_is_rejected() {
        let json = with(
            "4efe172df014693732e8b64a004cf382b8599eaf86be26a9b38c0d2993e60e1d",
            "XYZ",
        );
        let error = Manifest::parse(&json).expect_err("bad hash");
        assert!(error.to_string().contains("not a SHA-256"), "{error}");
    }

    #[test]
    fn a_manifest_with_an_unsafe_install_name_is_rejected() {
        let json = with(r#""to": "MediaInfo.dll""#, r#""to": "..\\MediaInfo.dll""#);
        let error = Manifest::parse(&json).expect_err("unsafe name");
        assert!(error.to_string().contains("plain file name"), "{error}");
    }

    #[test]
    fn a_manifest_missing_a_tool_file_is_rejected() {
        let json = with(
            r#""provides": "ffmpeg", "from": "ffmpeg", "to": "ffmpeg""#,
            r#""from": "ffmpeg", "to": "ffmpeg""#,
        );
        let error = Manifest::parse(&json).expect_err("no ffmpeg");
        assert!(
            error.to_string().contains("nothing provides ffmpeg"),
            "{error}"
        );
    }

    #[test]
    fn an_unknown_schema_is_rejected() {
        let error = Manifest::parse(&with("\"schema\": 1", "\"schema\": 9")).expect_err("schema");
        assert!(error.to_string().contains("schema 9"), "{error}");
    }
}
