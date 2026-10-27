//! The numbered name offered for a conflict: "Use Crash (2004) (2)".

use super::Disk;

/// The most numbers tried before giving up; no real library has this many twins.
const MAX_TRIES: u32 = 999;

/// The first free "Name (n)" for a destination that is taken, counting from 2.
///
/// `dest` is the destination relative to the data source `root`, with `/`. A name that already
/// ends in " (n)" (n from 2 to 99) continues from there: `Crash (2004) (2)` offers
/// `Crash (2004) (3)`. A candidate is free when nothing is on the disk at that path (with the
/// volume's own case rules, as the `disk` answers) and it is not in `taken`, the destinations
/// other titles of the same batch will use (compared without regard to case).
pub fn suggest_numbered(disk: &dyn Disk, root: &str, dest: &str, taken: &[&str]) -> String {
    let dest = dest.trim_end_matches('/');
    let (stem, first) = match split_number(dest) {
        Some((stem, n)) => (stem, n + 1),
        None => (dest, 2),
    };
    let free = |candidate: &str| {
        disk.occupant(root, candidate).is_none()
            && !taken
                .iter()
                .any(|other| other.eq_ignore_ascii_case(candidate))
    };
    for n in first..first + MAX_TRIES {
        let candidate = format!("{stem} ({n})");
        if free(&candidate) {
            return candidate;
        }
    }
    format!("{stem} ({})", first + MAX_TRIES)
}

/// `Crash (2004) (2)` splits into `Crash (2004)` and 2. A year in brackets is not a number.
fn split_number(name: &str) -> Option<(&str, u32)> {
    let open = name.rfind(" (")?;
    let digits = name[open + 2..].strip_suffix(')')?;
    if digits.is_empty() || digits.len() > 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    (n >= 2).then_some((&name[..open], n))
}

#[cfg(test)]
mod tests {
    use super::super::MemoryDisk;
    use super::*;

    const ROOT: &str = "D:/Movies";

    #[test]
    fn the_first_free_number_is_two_then_three() {
        let disk = MemoryDisk::new(true).with_dir(ROOT, "Crash (2004)", false, Some("a"));
        assert_eq!(
            suggest_numbered(&disk, ROOT, "Crash (2004)", &[]),
            "Crash (2004) (2)"
        );
        let disk = disk.with_dir(ROOT, "Crash (2004) (2)", false, Some("b"));
        assert_eq!(
            suggest_numbered(&disk, ROOT, "Crash (2004)", &[]),
            "Crash (2004) (3)"
        );
    }

    #[test]
    fn a_name_that_ends_in_a_number_continues_from_it() {
        let disk = MemoryDisk::new(true);
        assert_eq!(
            suggest_numbered(&disk, ROOT, "Crash (2004) (2)", &[]),
            "Crash (2004) (3)"
        );
        // A year is not a number.
        assert_eq!(
            suggest_numbered(&disk, ROOT, "Blade Runner (2049)", &[]),
            "Blade Runner (2049) (2)"
        );
        assert_eq!(
            suggest_numbered(&disk, ROOT, "Blade Runner 2049", &[]),
            "Blade Runner 2049 (2)"
        );
    }

    #[test]
    fn names_other_titles_of_the_batch_will_take_are_skipped() {
        let disk = MemoryDisk::new(false);
        assert_eq!(
            suggest_numbered(&disk, ROOT, "Crash (2004)", &["crash (2004) (2)"]),
            "Crash (2004) (3)"
        );
    }

    #[test]
    fn the_volume_decides_whether_case_matters() {
        let insensitive = MemoryDisk::new(true).with_dir(ROOT, "crash (2004) (2)", true, None);
        assert_eq!(
            suggest_numbered(&insensitive, ROOT, "Crash (2004)", &[]),
            "Crash (2004) (3)"
        );
        let sensitive = MemoryDisk::new(false).with_dir(ROOT, "crash (2004) (2)", true, None);
        assert_eq!(
            suggest_numbered(&sensitive, ROOT, "Crash (2004)", &[]),
            "Crash (2004) (2)"
        );
    }
}
