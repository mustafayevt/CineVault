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

/// `name` (a file or folder name) folded the way this platform compares names: lower-cased where
/// the volumes ignore case (Windows, macOS), unchanged on Linux. Compare folded names only with
/// each other.
#[must_use]
pub fn fold_name(name: &str) -> String {
    if CASE_SENSITIVE {
        name.to_owned()
    } else {
        name.to_lowercase()
    }
}

/// Whether the file or folder names `a` and `b` are the same name on this platform: equal
/// ignoring ASCII case where the volumes ignore case (Windows, macOS), exactly equal on Linux.
#[must_use]
pub fn same_name(a: &str, b: &str) -> bool {
    if CASE_SENSITIVE {
        a == b
    } else {
        a.eq_ignore_ascii_case(b)
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

/// Whether the folder `root` (a [`linux_mount_root`]) has its drive, read from the system's mount
/// list and `/etc/fstab` (see [`mount_root_answers_with`]).
#[cfg(target_os = "linux")]
#[must_use]
pub fn mount_root_answers(root: &Path) -> bool {
    if is_mount_point(root) {
        return true;
    }
    let read = |file: &str| std::fs::read_to_string(file).unwrap_or_default();
    mount_root_answers_with(
        &root.to_string_lossy(),
        root.is_dir(),
        &read("/proc/self/mountinfo"),
        &read("/etc/fstab"),
    )
}

/// Whether the folder `root` (a [`linux_mount_root`], not a mount point by device) has its drive,
/// given the text of `/proc/self/mountinfo` and of `/etc/fstab`:
///
/// * a folder the mount list names is there: a bind mount of a folder on the same file system
///   has no device of its own, but it is mounted;
/// * below `/media` and `/run/media` the system makes the folder for a removable drive, so one
///   that is not mounted is what an unplugged drive left behind;
/// * below `/mnt`, a folder `/etc/fstab` mounts a drive on is missing its drive; any other
///   folder there (made by hand, or a link to another folder) is used as it is, or it would read
///   as offline forever.
///
/// An unmounted drive's folder can hold files a program wrote there meanwhile, so its content
/// says nothing. Pure, so it is tested on every platform.
#[must_use]
pub fn mount_root_answers_with(root: &str, is_dir: bool, mountinfo: &str, fstab: &str) -> bool {
    let root = root.trim_end_matches('/');
    let names_root = |text: &str, field: usize| {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| line.split_whitespace().nth(field))
            .any(|point| unescape_octal(point).trim_end_matches('/') == root)
    };
    if !is_dir {
        return false;
    }
    if names_root(mountinfo, 4) {
        return true;
    }
    if root.starts_with("/media/") || root.starts_with("/run/media/") {
        return false;
    }
    !names_root(fstab, 1)
}

/// Decodes the `\040`-style octal escapes the mount list and `fstab` use for spaces, tabs and
/// backslashes in a path.
fn unescape_octal(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let digits = bytes
            .get(i + 1..i + 4)
            .filter(|digits| bytes[i] == b'\\' && digits.iter().all(|d| (b'0'..=b'7').contains(d)));
        if let Some(byte) = digits
            .map(|d| d.iter().fold(0u32, |acc, d| acc * 8 + u32::from(d - b'0')))
            .and_then(|value| u8::try_from(value).ok())
        {
            out.push(byte);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether the drive that holds `path` is really there. On Linux a path under `/media`,
/// `/run/media` or `/mnt` belongs to a mount point (see [`linux_mount_root`]); when the drive is
/// unplugged that folder stays behind on the system drive and must not pass for the drive
/// ([`mount_root_answers`]). True for any other path (the system drive is always there).
/// Always true off Linux.
#[must_use]
pub fn drive_is_mounted(path: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        let root = linux_mount_root(&path.to_string_lossy());
        root == "/" || mount_root_answers(Path::new(&root))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        true
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

    const MOUNTINFO: &str = "\
28 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw
40 28 8:2 /srv/films /mnt/films rw,relatime shared:1 - ext4 /dev/sda2 rw
41 28 8:17 / /media/alex/My\\040Disk rw,relatime - exfat /dev/sdb1 rw
";

    const FSTAB: &str = "\
# <file system> <mount point> <type> <options> <dump> <pass>
UUID=1234 / ext4 errors=remount-ro 0 1
nas:/export /mnt/nas nfs4 defaults,nofail 0 0
/srv/films /mnt/films none bind 0 0
UUID=5678 /mnt/Backup\\040Disk ext4 nofail 0 2
";

    #[test]
    fn a_bind_mount_and_a_listed_removable_drive_are_there() {
        // Same device as `/`, so no mount point by device, but the mount list names it.
        assert!(mount_root_answers_with(
            "/mnt/films",
            true,
            MOUNTINFO,
            FSTAB
        ));
        assert!(mount_root_answers_with(
            "/media/alex/My Disk",
            true,
            MOUNTINFO,
            FSTAB
        ));
    }

    #[test]
    fn the_folder_an_unplugged_drive_leaves_is_not_its_drive() {
        // A drive the system is told to mount at `/mnt`, not mounted now.
        assert!(!mount_root_answers_with("/mnt/nas", true, MOUNTINFO, FSTAB));
        assert!(!mount_root_answers_with(
            "/mnt/Backup Disk",
            true,
            MOUNTINFO,
            FSTAB
        ));
        // A removable drive's folder that nothing is mounted on.
        assert!(!mount_root_answers_with(
            "/media/alex/Films",
            true,
            MOUNTINFO,
            FSTAB
        ));
        assert!(!mount_root_answers_with(
            "/run/media/alex/Films",
            true,
            MOUNTINFO,
            FSTAB
        ));
        // No folder at all.
        assert!(!mount_root_answers_with(
            "/mnt/films",
            false,
            MOUNTINFO,
            FSTAB
        ));
    }

    #[test]
    fn a_plain_folder_or_link_below_mnt_is_used_as_it_is() {
        // Made by hand, or a link to a folder elsewhere: neither mount list names it.
        assert!(mount_root_answers_with(
            "/mnt/movies",
            true,
            MOUNTINFO,
            FSTAB
        ));
        // A commented line mounts nothing.
        assert!(mount_root_answers_with(
            "/mnt/movies",
            true,
            MOUNTINFO,
            "# nas:/export /mnt/movies nfs4 defaults 0 0\n"
        ));
        // With no fstab and no mount list, only a folder below `/mnt` is taken.
        assert!(mount_root_answers_with("/mnt/movies", true, "", ""));
        assert!(!mount_root_answers_with("/media/alex/Films", true, "", ""));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_root_answers_and_a_made_up_drive_does_not() {
        assert!(mount_root_answers(Path::new("/")));
        assert!(!mount_root_answers(Path::new(
            "/mnt/cinevault-test-drive-that-is-not-there"
        )));
    }

    #[test]
    fn a_drive_that_is_not_mounted_is_not_there() {
        let dir = tempfile::tempdir().expect("temp dir");
        // The system drive and a made-up drive.
        assert!(drive_is_mounted(dir.path()));
        assert!(drive_is_mounted(Path::new(r"D:\Movies")));
        let unplugged = Path::new("/mnt/cinevault-test-drive-that-is-not-there/Movies");
        assert_eq!(drive_is_mounted(unplugged), !cfg!(target_os = "linux"));
    }

    #[test]
    fn names_fold_and_compare_by_the_platform_rule() {
        assert_eq!(
            fold_name("Movie.MKV"),
            if CASE_SENSITIVE {
                "Movie.MKV"
            } else {
                "movie.mkv"
            }
        );
        assert!(same_name("Movie.mkv", "Movie.mkv"));
        assert!(!same_name("Movie.mkv", "Movies.mkv"));
        assert_eq!(same_name("Movie.mkv", "movie.MKV"), !CASE_SENSITIVE);
    }
}
