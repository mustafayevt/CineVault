//! Unpacking a verified download. A download of listed files writes only the files the manifest
//! names, always under the name the manifest gives them. A tree download writes everything below
//! one folder of the archive, and refuses anything that could land elsewhere: absolute paths, `..`,
//! drive letters, links, special files and entries outside the folder.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use super::ToolError;
use super::manifest::{ArchiveFormat, Artifact, ArtifactMode, FileSpec};

/// The largest single file taken from an archive of listed files. The biggest real one is about
/// 110 MB.
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// The most entries a tree may hold.
const MAX_TREE_FILES: usize = 20_000;

/// A file an unpack wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Extracted {
    /// Its path in the install folder, with `/` separators.
    pub name: String,
    /// The tool it is, if it is one.
    pub tool: Option<String>,
}

/// Writes what `artifact` asks for from `archive` into `dest_dir` (which exists) and lists the
/// files written.
///
/// # Errors
/// Fails when the archive cannot be read, lacks a listed file, holds an oversized or unsafe entry
/// or is larger than its limit unpacked.
pub(crate) fn extract(
    artifact: &Artifact,
    archive: &Path,
    dest_dir: &Path,
) -> Result<Vec<Extracted>, ToolError> {
    match artifact.mode {
        ArtifactMode::Tree => extract_tree(artifact, archive, dest_dir),
        ArtifactMode::Files => {
            extract_files(artifact.format, archive, &artifact.files, dest_dir)?;
            Ok(artifact
                .files
                .iter()
                .map(|file| Extracted {
                    name: file.to.clone(),
                    tool: file.provides.clone(),
                })
                .collect())
        }
    }
}

fn extract_files(
    format: ArchiveFormat,
    archive: &Path,
    files: &[FileSpec],
    dest_dir: &Path,
) -> Result<(), ToolError> {
    match format {
        ArchiveFormat::Raw => extract_raw(archive, files, dest_dir),
        ArchiveFormat::Zip => extract_zip(archive, files, dest_dir),
        ArchiveFormat::TarBz2 => {
            let file = open(archive)?;
            extract_tar(bzip2::read::BzDecoder::new(file), files, dest_dir)
        }
        ArchiveFormat::TarGz => {
            let file = open(archive)?;
            extract_tar(flate2::read::GzDecoder::new(file), files, dest_dir)
        }
    }
}

fn archive_error(message: impl std::fmt::Display) -> ToolError {
    ToolError::Archive(message.to_string())
}

fn open(archive: &Path) -> Result<File, ToolError> {
    File::open(archive)
        .map_err(|error| ToolError::io(format!("Could not read {}", archive.display()), error))
}

/// An archive entry name with `/` separators and no leading `./`.
fn normalize(name: &str) -> String {
    let name = name.replace('\\', "/");
    name.trim_start_matches("./").to_owned()
}

fn write_entry(source: &mut dyn Read, dest: &Path, what: &str) -> Result<(), ToolError> {
    let mut file = File::create_new(dest)
        .map_err(|error| ToolError::io(format!("Could not create {}", dest.display()), error))?;
    // One byte more than allowed tells an oversized entry from one of exactly the limit.
    let copied = io::copy(&mut source.take(MAX_FILE_BYTES + 1), &mut file)
        .map_err(|error| archive_error(format!("{what}: {error}")))?;
    if copied > MAX_FILE_BYTES {
        return Err(archive_error(format!("{what} is unreasonably large")));
    }
    file.sync_all()
        .map_err(|error| ToolError::io(format!("Could not write {}", dest.display()), error))
}

fn extract_raw(archive: &Path, files: &[FileSpec], dest_dir: &Path) -> Result<(), ToolError> {
    let [spec] = files else {
        return Err(archive_error("a raw download holds exactly one file"));
    };
    let mut source = open(archive)?;
    write_entry(&mut source, &dest_dir.join(&spec.to), &spec.to)
}

fn extract_zip(archive: &Path, files: &[FileSpec], dest_dir: &Path) -> Result<(), ToolError> {
    let mut zip = zip::ZipArchive::new(open(archive)?).map_err(archive_error)?;
    for spec in files {
        let mut index = None;
        for i in 0..zip.len() {
            let entry = zip.by_index_raw(i).map_err(archive_error)?;
            if !entry.is_dir() && entry.name().is_ok_and(|name| normalize(&name) == spec.from) {
                index = Some(i);
                break;
            }
        }
        let index = index.ok_or_else(|| archive_error(format!("{} is missing", spec.from)))?;
        let mut entry = zip.by_index(index).map_err(archive_error)?;
        write_entry(&mut entry, &dest_dir.join(&spec.to), &spec.from)?;
    }
    Ok(())
}

fn extract_tar(reader: impl Read, files: &[FileSpec], dest_dir: &Path) -> Result<(), ToolError> {
    let mut tar = tar::Archive::new(reader);
    let mut found = BTreeSet::new();
    for entry in tar.entries().map_err(archive_error)? {
        let mut entry = entry.map_err(archive_error)?;
        // Links are never followed: only regular files are taken.
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = normalize(&entry.path().map_err(archive_error)?.to_string_lossy());
        let Some(spec) = files.iter().find(|spec| spec.from == name) else {
            continue;
        };
        if found.insert(spec.from.clone()) {
            write_entry(&mut entry, &dest_dir.join(&spec.to), &spec.from)?;
        }
        if found.len() == files.len() {
            break;
        }
    }
    match files.iter().find(|spec| !found.contains(&spec.from)) {
        Some(missing) => Err(archive_error(format!("{} is missing", missing.from))),
        None => Ok(()),
    }
}

// ---- tree installs ------------------------------------------------------------------------

/// What an archive entry is, as far as a tree install cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    File {
        executable: bool,
    },
    Dir,
    /// A link, device, pipe or anything else that is not plain content.
    Other,
}

/// The place a tree is written to, and what it has used so far.
struct TreeWriter<'a> {
    dest: &'a Path,
    root: String,
    limit: u64,
    used: u64,
    files: Vec<Extracted>,
    program: &'a str,
    program_tool: &'a str,
}

impl TreeWriter<'_> {
    /// The path of `name` below the root, or `None` for the root folder itself.
    ///
    /// # Errors
    /// An unsafe name, or one outside the root.
    fn relative(&self, name: &str) -> Result<Option<String>, ToolError> {
        let unsafe_entry = |why: &str| archive_error(format!("the entry {name:?} {why}"));
        let normal = normalize(name);
        if normal.is_empty() || normal.contains('\0') {
            return Err(unsafe_entry("has no usable name"));
        }
        if normal.starts_with('/') {
            return Err(unsafe_entry("is an absolute path"));
        }
        let trimmed = normal.trim_end_matches('/');
        if trimmed
            .split('/')
            .any(|part| part == ".." || part == "." || part.is_empty())
        {
            return Err(unsafe_entry("climbs out of its folder"));
        }
        if trimmed
            .split('/')
            .any(|part| part.contains([':', '*', '?', '"', '<', '>', '|']))
        {
            return Err(unsafe_entry("has a name that cannot be used"));
        }
        let below = if self.root.is_empty() {
            trimmed
        } else if trimmed == self.root.trim_end_matches('/') {
            return Ok(None);
        } else {
            trimmed
                .strip_prefix(self.root.as_str())
                .ok_or_else(|| unsafe_entry("is outside the folder that is installed"))?
        };
        Ok(Some(below.to_owned()))
    }

    fn add(
        &mut self,
        name: &str,
        kind: Kind,
        source: &mut dyn Read,
        size_hint: Option<u64>,
    ) -> Result<(), ToolError> {
        let Some(relative) = self.relative(name)? else {
            return match kind {
                Kind::Dir => Ok(()),
                _ => Err(archive_error(format!("the entry {name:?} is not a folder"))),
            };
        };
        let target = self.dest.join(&relative);
        match kind {
            Kind::Other => Err(archive_error(format!(
                "the entry {name:?} is a link or a special file, which is not allowed"
            ))),
            Kind::Dir => fs::create_dir_all(&target).map_err(|error| {
                ToolError::io(format!("Could not create {}", target.display()), error)
            }),
            Kind::File { executable } => {
                if self.files.len() >= MAX_TREE_FILES {
                    return Err(archive_error("the download holds too many files"));
                }
                if size_hint.is_some_and(|size| size > self.limit.saturating_sub(self.used)) {
                    return Err(too_big(self.limit));
                }
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        ToolError::io(format!("Could not create {}", parent.display()), error)
                    })?;
                }
                let mut file = File::create_new(&target).map_err(|error| {
                    ToolError::io(format!("Could not create {}", target.display()), error)
                })?;
                let room = self.limit - self.used;
                // One byte more than the room left tells "exactly at the limit" from "over it".
                let copied = io::copy(&mut source.take(room + 1), &mut file)
                    .map_err(|error| archive_error(format!("{name}: {error}")))?;
                if copied > room {
                    return Err(too_big(self.limit));
                }
                self.used += copied;
                file.sync_all().map_err(|error| {
                    ToolError::io(format!("Could not write {}", target.display()), error)
                })?;
                drop(file);
                set_unix_mode(&target, executable);
                let tool = (relative == self.program).then(|| self.program_tool.to_owned());
                self.files.push(Extracted {
                    name: relative,
                    tool,
                });
                Ok(())
            }
        }
    }

    fn finish(self) -> Result<Vec<Extracted>, ToolError> {
        if self.files.iter().any(|file| file.tool.is_some()) {
            Ok(self.files)
        } else {
            Err(archive_error(format!("{} is missing", self.program)))
        }
    }
}

fn too_big(limit: u64) -> ToolError {
    archive_error(format!(
        "the download unpacks to more than {} MB, which is more than expected",
        limit / (1024 * 1024)
    ))
}

#[cfg(unix)]
fn set_unix_mode(path: &Path, executable: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_unix_mode(_path: &Path, _executable: bool) {}

fn extract_tree(
    artifact: &Artifact,
    archive: &Path,
    dest_dir: &Path,
) -> Result<Vec<Extracted>, ToolError> {
    let program = artifact
        .program
        .as_ref()
        .ok_or_else(|| archive_error("a tree download names no program"))?;
    let mut writer = TreeWriter {
        dest: dest_dir,
        root: artifact.tree_root(),
        limit: artifact.unpack_limit(),
        used: 0,
        files: Vec::new(),
        program: &program.path,
        program_tool: &program.provides,
    };
    match artifact.format {
        ArchiveFormat::Zip => tree_from_zip(archive, &mut writer)?,
        ArchiveFormat::TarBz2 => {
            tree_from_tar(bzip2::read::BzDecoder::new(open(archive)?), &mut writer)?;
        }
        ArchiveFormat::TarGz => {
            tree_from_tar(flate2::read::GzDecoder::new(open(archive)?), &mut writer)?;
        }
        ArchiveFormat::Raw => return Err(archive_error("a raw download cannot be a tree")),
    }
    writer.finish()
}

fn tree_from_zip(archive: &Path, writer: &mut TreeWriter<'_>) -> Result<(), ToolError> {
    let mut zip = zip::ZipArchive::new(open(archive)?).map_err(archive_error)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(archive_error)?;
        let name = entry.name().map_err(archive_error)?.to_string();
        let mode = entry.unix_mode();
        let linked = mode.is_some_and(|mode| {
            matches!(
                mode & 0o170_000,
                0o120_000 | 0o010_000 | 0o020_000 | 0o060_000 | 0o140_000
            )
        });
        let kind = if linked {
            Kind::Other
        } else if entry.is_dir() {
            Kind::Dir
        } else {
            Kind::File {
                executable: mode.is_some_and(|mode| mode & 0o111 != 0),
            }
        };
        let size = entry.size();
        writer.add(&name, kind, &mut entry, Some(size))?;
    }
    Ok(())
}

fn tree_from_tar(reader: impl Read, writer: &mut TreeWriter<'_>) -> Result<(), ToolError> {
    let mut tar = tar::Archive::new(reader);
    for entry in tar.entries().map_err(archive_error)? {
        let mut entry = entry.map_err(archive_error)?;
        let kind = {
            let header = entry.header();
            let entry_type = header.entry_type();
            if entry_type.is_dir() {
                Kind::Dir
            } else if entry_type.is_file() {
                Kind::File {
                    executable: header.mode().is_ok_and(|mode| mode & 0o111 != 0),
                }
            } else if entry_type.is_pax_global_extensions() || entry_type.is_pax_local_extensions()
            {
                continue;
            } else {
                Kind::Other
            }
        };
        let name = entry
            .path()
            .map_err(archive_error)?
            .to_string_lossy()
            .into_owned();
        let size = entry.size();
        writer.add(&name, kind, &mut entry, Some(size))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_names_are_normalised() {
        assert_eq!(normalize("./a/b"), "a/b");
        assert_eq!(normalize("a\\b\\c.exe"), "a/b/c.exe");
    }

    fn writer(root: &str, dest: &Path) -> TreeWriter<'static> {
        // Leaked so the test needs no lifetimes: it is a few bytes in a test process.
        let dest: &'static Path = Box::leak(dest.to_path_buf().into_boxed_path());
        TreeWriter {
            dest,
            root: root.to_owned(),
            limit: 1000,
            used: 0,
            files: Vec::new(),
            program: "run",
            program_tool: "indexer-service",
        }
    }

    #[test]
    fn names_are_resolved_below_the_root_or_refused() {
        let temp = tempfile::tempdir().expect("temp dir");
        let w = writer("Pkg/", temp.path());
        assert_eq!(
            w.relative("Pkg/a/b.txt").expect("ok").as_deref(),
            Some("a/b.txt")
        );
        assert_eq!(
            w.relative("./Pkg\\a\\b.txt").expect("ok").as_deref(),
            Some("a/b.txt")
        );
        assert_eq!(w.relative("Pkg/").expect("root"), None);
        assert_eq!(w.relative("Pkg").expect("root"), None);
        for bad in [
            "/etc/passwd",
            "Pkg/../x",
            "Pkg/a/../../x",
            "../Pkg/x",
            "Other/x",
            "PkgOther/x",
            "C:/x",
            "Pkg/C:/x",
            "Pkg//x",
            "Pkg/a<b",
        ] {
            assert!(w.relative(bad).is_err(), "{bad} was accepted");
        }
    }

    #[test]
    fn without_a_root_everything_is_below_it() {
        let temp = tempfile::tempdir().expect("temp dir");
        let w = writer("", temp.path());
        assert_eq!(w.relative("a/b").expect("ok").as_deref(), Some("a/b"));
        assert!(w.relative("a/../b").is_err());
    }
}
