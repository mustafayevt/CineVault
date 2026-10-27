//! The result of planning: what happens to each file, as data.
//!
//! A plan never touches the disk. It lists, for every file the item owns, where it goes
//! ([`Fate`]), and derives the operations an executor performs ([`Op`]). All paths are relative
//! to the root of the data source and use `/`.

use std::collections::BTreeMap;

use cv_domain::MediaFileType;

/// Where a rendered title folder goes when the profile keeps sub-folders.
///
/// `rendered` is the folder the pattern gives and `old_folder` the title's current folder, both
/// relative to the data source root. A pattern of one segment is placed in the folder that holds
/// the title now: the parent of its own folder, or `old_folder` itself when that folder is shared
/// by several titles (`container` true; the title has no folder of its own). A title already in a
/// folder of the rendered name stays there. A pattern with `/` builds its own structure from the
/// root (`M/Matrix (1999)`), so it is returned as it is: keeping the old parent there would nest
/// the letter folder again on every rename.
pub(crate) fn keep_subfolder(rendered: &str, old_folder: &str, container: bool) -> String {
    if rendered.is_empty() || rendered.contains('/') {
        return rendered.to_owned();
    }
    let (parent, own) = old_folder.rsplit_once('/').unwrap_or(("", old_folder));
    // A folder that differs from the rendered name only in case is the same folder on Windows.
    let parent = if container && own.to_lowercase() != rendered.to_lowercase() {
        old_folder
    } else {
        parent
    };
    if parent.is_empty() {
        rendered.to_owned()
    } else {
        format!("{parent}/{rendered}")
    }
}

/// A file that belongs to the item being planned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanFile {
    /// The path relative to the data source root.
    pub path: String,
    pub kind: MediaFileType,
    /// Modification time, epoch milliseconds. Of several files of one kind, the newest is kept.
    pub modified_ms: i64,
    /// The stacking marker of a video or subtitle (`cd1`, `part2`, `1of2`), empty if not stacked.
    pub stacking_marker: String,
    /// The part number of a stacked file; `0` when it is not stacked.
    pub stacking: u32,
    /// What the subtitle stream of a subtitle file says. `None` when it is not known.
    pub subtitle: Option<SubtitleTag>,
    /// For an NFO: it can be read as an NFO of this kind of item. A file that cannot is left alone
    /// (or moved to the trash when NFO clean-up is on).
    pub nfo_valid: bool,
}

impl PlanFile {
    pub fn new(path: impl Into<String>, kind: MediaFileType) -> Self {
        PlanFile {
            path: path.into(),
            kind,
            modified_ms: 0,
            stacking_marker: String::new(),
            stacking: 0,
            subtitle: None,
            nfo_valid: true,
        }
    }

    #[must_use]
    pub fn modified(mut self, modified_ms: i64) -> Self {
        self.modified_ms = modified_ms;
        self
    }

    #[must_use]
    pub fn stacked(mut self, marker: impl Into<String>, number: u32) -> Self {
        self.stacking_marker = marker.into();
        self.stacking = number;
        self
    }

    #[must_use]
    pub fn subtitle(mut self, tag: SubtitleTag) -> Self {
        self.subtitle = Some(tag);
        self
    }

    #[must_use]
    pub fn invalid_nfo(mut self) -> Self {
        self.nfo_valid = false;
        self
    }

    /// The folder part of the path (`""` for a file in the data source root).
    pub fn parent(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(parent, _)| parent)
    }

    /// The file name with its extension.
    pub fn name(&self) -> &str {
        self.path
            .rsplit_once('/')
            .map_or(&self.path, |(_, name)| name)
    }
}

/// What a subtitle file says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubtitleTag {
    /// The language as the stream stores it (`en`, `pt-BR`); empty when unknown.
    pub language: String,
    pub forced: bool,
    /// For the hearing impaired.
    pub sdh: bool,
    /// The stream title, written after the flags.
    pub title: String,
}

/// What happens to one file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Fate {
    /// The file goes to these paths: moved to the first, copied to the others. When the file
    /// is already at its first destination nothing is done for that one. If the file's own path
    /// is among the destinations it comes first.
    Carried { to: Vec<String> },
    /// The file is no longer needed: it goes to the trash folder.
    Trash,
    /// The file stays where it is (an NFO this item cannot read, for example).
    LeftBehind,
}

/// Who a file belongs to in a TV plan.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Owner {
    Movie,
    Show,
    Season(u32),
    /// The episodes that share one video file; the index is that of the first of them in the
    /// input.
    Episodes(usize),
}

/// One file and its fate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlannedFile {
    pub from: String,
    pub kind: MediaFileType,
    pub owner: Owner,
    pub fate: Fate,
}

impl PlannedFile {
    /// True when the file is neither moved, copied nor trashed.
    pub fn is_unchanged(&self) -> bool {
        match &self.fate {
            Fate::Carried { to } => to.len() == 1 && to[0] == self.from,
            Fate::Trash => false,
            Fate::LeftBehind => true,
        }
    }
}

/// What happens to the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FolderChange {
    /// The folder keeps its path.
    Unchanged,
    /// The whole folder is renamed or moved; every file in it goes along.
    Rename,
    /// Files are moved one by one: the old folder is shared with other movies, the new one is
    /// shared, or some file stays behind.
    MoveFiles,
}

/// The kind of an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Move,
    Copy,
    /// Write a fresh NFO at `to` (no source file).
    WriteNfo,
    Trash,
}

/// One step an executor performs. `from` is a current path, `to` a final path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    pub kind: OpKind,
    pub file_type: MediaFileType,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// Something the preview should show next to the item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// An NFO that cannot be read as an NFO of this item is not renamed.
    UnreadableNfo { path: String },
    /// Two different files would end up at the same path.
    DuplicateDestination { path: String },
    /// An episode pattern without an episode token is refused.
    EpisodePatternWithoutEpisode,
}

/// Builds the operations of a list of planned files: copies first, then the move, then the
/// trash entries. Files that stay where they are produce no operation.
pub(crate) fn ops_of(files: &[PlannedFile]) -> Vec<Op> {
    let mut ops = Vec::new();
    for file in files {
        match &file.fate {
            Fate::Carried { to } => {
                for target in to.iter().skip(1) {
                    ops.push(Op {
                        kind: OpKind::Copy,
                        file_type: file.kind,
                        from: Some(file.from.clone()),
                        to: Some(target.clone()),
                    });
                }
                if let Some(first) = to.first()
                    && *first != file.from
                {
                    ops.push(Op {
                        kind: OpKind::Move,
                        file_type: file.kind,
                        from: Some(file.from.clone()),
                        to: Some(first.clone()),
                    });
                }
            }
            Fate::Trash => ops.push(Op {
                kind: OpKind::Trash,
                file_type: file.kind,
                from: Some(file.from.clone()),
                to: None,
            }),
            Fate::LeftBehind => {}
        }
    }
    ops
}

/// The destinations that more than one source file claims, sorted.
pub(crate) fn duplicate_destinations(files: &[PlannedFile]) -> Vec<String> {
    let mut claims: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for file in files {
        if let Fate::Carried { to } = &file.fate {
            for target in to {
                let sources = claims.entry(target).or_default();
                if !sources.contains(&file.from.as_str()) {
                    sources.push(&file.from);
                }
            }
        }
    }
    claims
        .into_iter()
        .filter(|(_, sources)| sources.len() > 1)
        .map(|(path, _)| path.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carried(from: &str, to: &[&str]) -> PlannedFile {
        PlannedFile {
            from: from.to_owned(),
            kind: MediaFileType::Poster,
            owner: Owner::Movie,
            fate: Fate::Carried {
                to: to.iter().map(|t| (*t).to_owned()).collect(),
            },
        }
    }

    #[test]
    fn ops_copy_first_then_move_and_skip_files_in_place() {
        let files = [
            carried("a/old.jpg", &["b/new.jpg", "b/copy.jpg"]),
            carried("a/same.jpg", &["a/same.jpg"]),
            carried("a/keep.jpg", &["a/keep.jpg", "a/other.jpg"]),
            PlannedFile {
                from: "a/gone.txt".to_owned(),
                kind: MediaFileType::Text,
                owner: Owner::Movie,
                fate: Fate::Trash,
            },
        ];
        let ops = ops_of(&files);
        let summary: Vec<(OpKind, &str, Option<&str>)> = ops
            .iter()
            .map(|op| (op.kind, op.from.as_deref().unwrap(), op.to.as_deref()))
            .collect();
        assert_eq!(
            summary,
            [
                (OpKind::Copy, "a/old.jpg", Some("b/copy.jpg")),
                (OpKind::Move, "a/old.jpg", Some("b/new.jpg")),
                (OpKind::Copy, "a/keep.jpg", Some("a/other.jpg")),
                (OpKind::Trash, "a/gone.txt", None),
            ]
        );
        assert!(files[1].is_unchanged());
        assert!(!files[0].is_unchanged());
    }

    #[test]
    fn duplicates_are_destinations_of_two_sources() {
        let files = [
            carried("a/x.jpg", &["n/poster.jpg"]),
            carried("a/y.jpg", &["n/poster.jpg", "n/other.jpg"]),
            carried("a/z.jpg", &["n/z.jpg", "n/z.jpg"]),
        ];
        assert_eq!(duplicate_destinations(&files), ["n/poster.jpg"]);
    }

    #[test]
    fn plan_file_paths() {
        let f = PlanFile::new("Movies/Heat/Heat.mkv", MediaFileType::Video);
        assert_eq!(f.parent(), "Movies/Heat");
        assert_eq!(f.name(), "Heat.mkv");
        let root = PlanFile::new("Heat.mkv", MediaFileType::Video);
        assert_eq!(root.parent(), "");
        assert_eq!(root.name(), "Heat.mkv");
    }
}
