//! Local-time text for NFO files: `yyyy-MM-dd HH:mm:ss` in the computer's time zone.

use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Local, NaiveDateTime, Utc};

const FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// An instant in epoch milliseconds as local text. `None` for zero or negative values (unset).
pub fn local_from_millis(ms: i64) -> Option<String> {
    if ms <= 0 {
        return None;
    }
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|t| t.with_timezone(&Local).format(FORMAT).to_string())
}

/// Local `yyyy-MM-dd HH:mm:ss` text as epoch milliseconds. `None` when it does not parse (or the
/// local time does not exist, as in a clock change).
pub fn millis_from_local(text: &str) -> Option<i64> {
    let naive = NaiveDateTime::parse_from_str(text.trim(), FORMAT).ok()?;
    let local = naive.and_local_timezone(Local).earliest()?;
    Some(local.timestamp_millis())
}

/// A file-system time as local text.
pub fn local_from_system(time: SystemTime) -> String {
    DateTime::<Local>::from(time).format(FORMAT).to_string()
}

/// The current local time.
pub fn local_now() -> String {
    Local::now().format(FORMAT).to_string()
}

/// A file's creation and modification times as local text. Where the file system keeps no
/// creation time, the modification time stands in for it. Both are `None` when the file cannot be
/// read.
pub fn file_times(path: &Path) -> (Option<String>, Option<String>) {
    let Ok(meta) = std::fs::metadata(path) else {
        return (None, None);
    };
    let modified = meta.modified().ok().map(local_from_system);
    let created = meta
        .created()
        .ok()
        .map(local_from_system)
        .or_else(|| modified.clone());
    (created, modified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_times_give_nothing() {
        assert_eq!(local_from_millis(0), None);
        assert_eq!(local_from_millis(-5), None);
    }

    #[test]
    fn a_time_has_the_nfo_layout() {
        let text = local_from_millis(1_700_000_000_000).unwrap();
        assert_eq!(text.len(), 19);
        assert_eq!(&text[4..5], "-");
        assert_eq!(&text[10..11], " ");
        assert_eq!(&text[13..14], ":");
        assert_eq!(local_now().len(), 19);
    }

    #[test]
    fn local_text_round_trips() {
        let ms = 1_700_000_000_000;
        assert_eq!(millis_from_local(&local_from_millis(ms).unwrap()), Some(ms));
        assert_eq!(millis_from_local("not a date"), None);
    }

    #[test]
    fn file_times_come_from_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mkv");
        std::fs::write(&path, b"x").unwrap();
        let (created, modified) = file_times(&path);
        assert!(created.is_some() && modified.is_some());
        assert_eq!(file_times(&dir.path().join("none.mkv")), (None, None));
    }
}
