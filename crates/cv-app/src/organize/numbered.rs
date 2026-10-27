//! The numbered folder names offered for a conflict ("Use Crash (2004) (2)").

use cv_rename::conflict::{Disk, FsDisk, suggest_numbered};

use super::{ConflictRow, Preview};

/// For each item of the conflict, in order, the first free "Name (n)" for its new folder: free on
/// the disk and not the new folder of another title of the preview. Empty for an item the preview
/// has no row for.
#[must_use]
pub fn numbered_names(preview: &Preview, conflict: &ConflictRow) -> Vec<String> {
    numbered_names_on(&FsDisk::new(), preview, conflict)
}

/// [`numbered_names`] over any [`Disk`].
#[must_use]
pub fn numbered_names_on(
    disk: &dyn Disk,
    preview: &Preview,
    conflict: &ConflictRow,
) -> Vec<String> {
    let mut offered: Vec<String> = Vec::new();
    let mut names = Vec::with_capacity(conflict.items.len());
    for id in &conflict.items {
        let Some(row) = preview.rows.iter().find(|row| &row.id == id) else {
            names.push(String::new());
            continue;
        };
        // Twins of one conflict get different numbers, so using both names resolves it.
        let taken: Vec<&str> = preview
            .rows
            .iter()
            .filter(|other| other.id != row.id && other.root == row.root)
            .map(|other| other.after.as_str())
            .chain(offered.iter().map(String::as_str))
            .collect();
        let name = suggest_numbered(disk, &row.root, &row.after, &taken);
        offered.push(name.clone());
        names.push(name);
    }
    names
}

#[cfg(test)]
mod tests {
    use cv_rename::conflict::MemoryDisk;

    use super::*;
    use crate::organize::{
        CompanionCounts, ConflictCode, PreviewRow, ResolutionOption, RowKind, RowStatus,
    };

    const ROOT: &str = "D:/Movies/";

    fn row(id: &str, after: &str) -> PreviewRow {
        PreviewRow {
            id: id.to_owned(),
            kind: RowKind::Movie,
            title: "Crash".to_owned(),
            year: 2004,
            root: ROOT.to_owned(),
            before: format!("crash {id}"),
            after: after.to_owned(),
            before_file: String::new(),
            after_file: String::new(),
            status: RowStatus::Conflict,
            moves: 0,
            trashed: 0,
            companions: CompanionCounts::default(),
            files: Vec::new(),
        }
    }

    fn conflict(items: &[&str]) -> ConflictRow {
        ConflictRow {
            code: ConflictCode::Duplicate,
            items: items.iter().map(|id| (*id).to_owned()).collect(),
            titles: items.iter().map(|_| "Crash".to_owned()).collect(),
            folders: Vec::new(),
            path: "Crash (2004)".to_owned(),
            options: vec![ResolutionOption::Skip],
        }
    }

    #[test]
    fn twins_get_different_numbers_and_an_existing_number_is_skipped() {
        let preview = Preview {
            rows: vec![row("a", "Crash (2004)"), row("b", "Crash (2004)")],
            ..Preview::default()
        };
        let disk = MemoryDisk::new(true).with_dir(ROOT, "Crash (2004) (2)", false, Some("c"));
        let names = numbered_names_on(&disk, &preview, &conflict(&["a", "b", "gone"]));
        // The disk has (2) already; the twins get (3) and (4), so using both resolves the pair.
        assert_eq!(names, ["Crash (2004) (3)", "Crash (2004) (4)", ""]);
    }

    #[test]
    fn a_name_another_title_will_take_is_not_offered() {
        let preview = Preview {
            rows: vec![row("a", "Crash (2004)"), row("b", "Crash (2004) (2)")],
            ..Preview::default()
        };
        let names = numbered_names_on(&MemoryDisk::new(true), &preview, &conflict(&["a"]));
        assert_eq!(names, ["Crash (2004) (3)"]);
    }
}
