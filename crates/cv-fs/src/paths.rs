//! Path identity: when two paths name the same file, and whether a path lies inside a folder.
//!
//! Windows and macOS find files without regard to case, so their keys fold case. Linux file
//! systems are case-sensitive: `Movie.mkv` and `movie.mkv` are two files there, so their keys keep
//! the case. Separators and the `\\?\` prefix are normalised on every platform, so `D:\Movies\` and
//! `d:/movies` agree on Windows.
//!
//! Keys are for comparing only. A name that is not UTF-8 is compared lossily (its invalid bytes
//! become U+FFFD), so two such names can compare equal.

use std::path::Path;

/// Whether the platform's file names are case-sensitive: true on Linux, false on Windows and macOS,
/// whose default volumes fold case.
pub const CASE_SENSITIVE: bool = cfg!(target_os = "linux");

/// The comparison key of `path` where names are case-sensitive when `case_sensitive` is true.
///
/// `\` becomes `/`. A `\\?\` prefix is dropped, and `\\?\UNC\server\share` becomes
/// `//server/share`, the same key as `\\server\share`. Trailing `/` is removed, but a lone `/`
/// stays. Without case sensitivity the text is lower-cased.
#[must_use]
pub fn key_with(path: &str, case_sensitive: bool) -> String {
    let swapped = path.replace('\\', "/");
    let unprefixed = swapped.strip_prefix("//?/").unwrap_or(&swapped);
    let unc = match unprefixed.strip_prefix("UNC/") {
        Some(rest) => format!("//{rest}"),
        None => unprefixed.to_owned(),
    };
    let trimmed = unc.trim_end_matches('/');
    let trimmed = if trimmed.is_empty() && !unc.is_empty() {
        "/"
    } else {
        trimmed
    };
    if case_sensitive {
        trimmed.to_owned()
    } else {
        trimmed.to_lowercase()
    }
}

/// The comparison key of `path` on this platform (see [`CASE_SENSITIVE`]).
#[must_use]
pub fn key(path: &Path) -> String {
    key_with(&path.to_string_lossy(), CASE_SENSITIVE)
}

/// Whether `a` and `b` name the same path on this platform.
#[must_use]
pub fn same_path(a: &Path, b: &Path) -> bool {
    key(a) == key(b)
}

/// Whether `path` is `folder` or lies below it, compared by whole components on this platform.
/// An empty `folder` contains nothing.
#[must_use]
pub fn is_within(path: &Path, folder: &Path) -> bool {
    within_keys(&key(path), &key(folder))
}

fn within_keys(path: &str, folder: &str) -> bool {
    if folder.is_empty() {
        return false;
    }
    if path == folder {
        return true;
    }
    // A key only ends in `/` when it is the root, which already ends with the separator.
    let prefix = if folder.ends_with('/') {
        folder.to_owned()
    } else {
        format!("{folder}/")
    };
    path.starts_with(&prefix)
}

/// The mount point that a Linux path under `/media`, `/run/media` or `/mnt` is expected to sit on,
/// judged by the path alone: `/media/<user>/<name>`, `/run/media/<user>/<name>` or `/mnt/<name>`.
/// Any other path, or one without the name component, gives `/`.
///
/// This is a pure text rule. Use [`is_mount_point`] to check the disk.
#[must_use]
pub fn linux_mount_root(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let kept = match parts.as_slice() {
        ["media", _, _, ..] => 3,
        ["run", "media", _, _, ..] => 4,
        ["mnt", _, ..] => 2,
        _ => return "/".to_owned(),
    };
    format!("/{}", parts[..kept].join("/"))
}

/// Whether `path` is the root of a file system: `/`, or a folder on another device than its
/// parent. False when either folder cannot be read.
#[cfg(unix)]
#[must_use]
pub fn is_mount_point(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Some(parent) = path.parent() else {
        return path == Path::new("/");
    };
    match (std::fs::metadata(path), std::fs::metadata(parent)) {
        (Ok(own), Ok(up)) => own.dev() != up.dev(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_is_kept_on_linux_and_folded_elsewhere() {
        assert_ne!(
            key_with("Movie.mkv", true),
            key_with("movie.mkv", true),
            "case-sensitive keys must tell the two names apart"
        );
        assert_eq!(
            key_with("Movie.mkv", false),
            key_with("movie.mkv", false),
            "case-folding keys must match the two names"
        );
    }

    #[test]
    fn windows_separators_and_trailing_slashes_agree() {
        assert_eq!(key_with(r"D:\Movies\", true), "D:/Movies");
        assert_eq!(key_with(r"D:\Movies\", false), key_with("d:/movies", false));
        assert_eq!(key_with("d:/movies", false), "d:/movies");
    }

    #[test]
    fn extended_prefixes_are_dropped() {
        assert_eq!(key_with(r"\\?\D:\x", false), key_with(r"D:\x", false));
        assert_eq!(key_with(r"\\?\D:\x", true), "D:/x");
        assert_eq!(
            key_with(r"\\?\UNC\nas\media\x", true),
            key_with(r"\\nas\media\x", true)
        );
        assert_eq!(key_with(r"\\?\UNC\nas\media\x", true), "//nas/media/x");
    }

    #[test]
    fn a_lone_slash_is_kept() {
        assert_eq!(key_with("/", true), "/");
        assert_eq!(key_with("/a/", true), "/a");
        assert_eq!(key_with("", true), "");
    }

    #[test]
    fn same_path_follows_the_platform_rule() {
        assert_eq!(
            same_path(Path::new("/x/Movie.mkv"), Path::new("/x/movie.mkv")),
            !CASE_SENSITIVE
        );
        assert!(same_path(Path::new(r"D:\x\"), Path::new("D:/x")));
        assert!(same_path(Path::new("/a/b/"), Path::new("/a/b")));
    }

    #[test]
    fn is_within_compares_whole_components() {
        assert!(!is_within(Path::new("/a/bc"), Path::new("/a/b")));
        assert!(is_within(Path::new("/a/b/c"), Path::new("/a/b")));
        assert!(is_within(Path::new("/a/b"), Path::new("/a/b")));
        assert!(is_within(Path::new("/a"), Path::new("/")));
        assert!(!is_within(Path::new("/a"), Path::new("")));
        assert!(!is_within(Path::new("/"), Path::new("/a")));
        assert!(is_within(
            Path::new(r"D:\Movies\Heat\x.mkv"),
            Path::new(r"D:\Movies")
        ));
        assert!(!is_within(
            Path::new(r"D:\Movies2\x"),
            Path::new(r"D:\Movies")
        ));
        assert_eq!(
            is_within(Path::new(r"D:\MOVIES\x"), Path::new(r"d:\movies")),
            !CASE_SENSITIVE
        );
    }

    #[test]
    fn linux_mount_roots_keep_the_drive_name() {
        assert_eq!(
            linux_mount_root("/media/alex/Films/Movies/x.mkv"),
            "/media/alex/Films"
        );
        assert_eq!(
            linux_mount_root("/run/media/alex/Films/TV"),
            "/run/media/alex/Films"
        );
        assert_eq!(linux_mount_root("/mnt/nas/Movies"), "/mnt/nas");
        assert_eq!(linux_mount_root("/home/alex/Movies"), "/");
    }

    #[test]
    fn paths_without_a_drive_name_or_elsewhere_give_the_root() {
        assert_eq!(linux_mount_root("/media/alex"), "/");
        assert_eq!(linux_mount_root("/media"), "/");
        assert_eq!(linux_mount_root("/mnt"), "/");
        assert_eq!(linux_mount_root("/run/media/alex"), "/");
        assert_eq!(linux_mount_root(r"D:\Movies"), "/");
        assert_eq!(linux_mount_root(""), "/");
    }

    #[cfg(unix)]
    #[test]
    fn the_root_is_a_mount_point() {
        assert!(is_mount_point(Path::new("/")));
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_inside_a_temp_folder_is_not_a_mount_point() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).expect("sub folder");
        assert!(!is_mount_point(&sub));
        assert!(!is_mount_point(&dir.path().join("missing")));
    }
}
