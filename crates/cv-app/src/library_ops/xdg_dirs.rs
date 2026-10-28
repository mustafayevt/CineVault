//! Which Trash folder, if any, can keep a path on Linux (the freedesktop Trash).
//!
//! The rule is the one in the design of the Linux support: an item goes to a Trash only by one
//! rename on its own file system, never by a copy. So a path is taken only when
//!
//! 1. it exists and its folder is on a local, writable file system (not a network file system,
//!    not mounted read-only), and
//! 2. a Trash folder on the same device can be used, in the freedesktop order: the home Trash
//!    (`$XDG_DATA_HOME/Trash`) when it is on that device, else `<mount>/.Trash/<uid>` when
//!    `<mount>/.Trash` is a real sticky folder, else `<mount>/.Trash-<uid>`.
//!
//! Whenever a fact cannot be read, or does not fit, the answer is "this volume cannot keep it":
//! the caller then moves the path to the library folder's own trash folder. Nothing here copies
//! or removes anything, and nothing here creates a folder (the sender does).
//!
//! The deciding part ([`parse_mountinfo`], [`entry_for`], [`choose_trash`]) is plain data and
//! compiled and tested everywhere; reading the disk is Linux only.

use std::path::PathBuf;

use super::bin_volumes::NoBin;

/// One line of `/proc/self/mountinfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct MountEntry {
    /// Where it is mounted, escapes decoded (`/media/alex/My Disk`).
    pub(crate) mount_point: String,
    /// `ext4`, `nfs4`, `fuse.sshfs`...
    pub(crate) fs_type: String,
    /// `ro` in the mount's own options or in the file system's options.
    pub(crate) read_only: bool,
    /// The device (`major:minor`) the kernel lists for the mount.
    pub(crate) dev: (u32, u32),
}

/// Decodes the `\040`-style octal escapes `mountinfo` uses for spaces, tabs, newlines and
/// backslashes in a path.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let octal = bytes
            .get(i + 1..i + 4)
            .filter(|digits| bytes[i] == b'\\' && digits.iter().all(|d| (b'0'..=b'7').contains(d)));
        if let Some(digits) = octal {
            let value = digits
                .iter()
                .fold(0u32, |acc, d| acc * 8 + u32::from(d - b'0'));
            // A value above 255 is no escape the kernel writes; keep the text as it is.
            if let Ok(byte) = u8::try_from(value) {
                out.push(byte);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The mounts in the text of `/proc/self/mountinfo`. A line that does not have the expected
/// shape is skipped.
///
/// A line reads: `id parent major:minor root mount-point options [optional fields...] - fstype
/// source super-options`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_mountinfo(text: &str) -> Vec<MountEntry> {
    text.lines().filter_map(parse_line).collect()
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_line(line: &str) -> Option<MountEntry> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let dash = fields.iter().position(|f| *f == "-")?;
    if dash < 6 {
        return None;
    }
    let (major, minor) = fields[2].split_once(':')?;
    let dev = (major.parse().ok()?, minor.parse().ok()?);
    let has_ro = |options: &str| options.split(',').any(|o| o == "ro");
    let super_options = fields.get(dash + 3).copied().unwrap_or("");
    Some(MountEntry {
        mount_point: unescape(fields[4]),
        fs_type: (*fields.get(dash + 1)?).to_owned(),
        // Either the mount or the file system being read-only means a rename cannot succeed.
        read_only: has_ro(fields[5]) || has_ro(super_options),
        dev,
    })
}

/// The mount that holds `path`: the one whose mount point is `path` or a folder above it,
/// the deepest first (compared part by part, case-sensitive). When two mounts share a mount
/// point, the later one in the list is on top and wins.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn entry_for<'a>(path: &str, entries: &'a [MountEntry]) -> Option<&'a MountEntry> {
    let parts = |text: &str| -> Vec<String> {
        text.split('/')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let wanted = parts(path);
    entries
        .iter()
        .filter(|entry| {
            let mount = parts(&entry.mount_point);
            wanted.len() >= mount.len() && wanted[..mount.len()] == mount[..]
        })
        // `max_by_key` keeps the last of equal keys: the mount on top.
        .max_by_key(|entry| parts(&entry.mount_point).len())
}

/// Whether a file system type is reached over a network (or a remote-storage mount), where a
/// rename into a Trash is not a promise that the item stays.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn is_network_fs(fs_type: &str) -> bool {
    const NETWORK: &[&str] = &[
        "nfs",
        "nfs4",
        "cifs",
        "smb3",
        "smbfs",
        "9p",
        "afs",
        "ceph",
        "glusterfs",
        "fuse.sshfs",
        "fuse.rclone",
        "davfs",
        "fuse.davfs2",
        "fuse.gvfsd-fuse",
        // More remote kinds; refusing one is always safe.
        "ncpfs",
        "lustre",
        "fuse.s3fs",
        "fuse.gcsfuse",
    ];
    NETWORK.contains(&fs_type)
}

/// The state of `<mount>/.Trash-<uid>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) enum OwnDir {
    /// A real folder of the user's, on this device, that the user can write to.
    Usable,
    /// It does not exist and the user can write to the mount point, so it can be created.
    Creatable,
    /// It exists but is a link, a file, another user's or not writable, or the mount point
    /// is not writable. It is never replaced.
    Unusable,
}

/// What the disk says about a path, as plain data for [`choose_trash`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct TrashFacts {
    /// The file system of the path is a network one.
    pub(crate) network: bool,
    /// The file system of the path is mounted read-only.
    pub(crate) read_only: bool,
    /// The home Trash folder, set only when it is on the device of the path and can be
    /// written to.
    pub(crate) home: Option<PathBuf>,
    /// The mount point of the path.
    pub(crate) topdir: PathBuf,
    /// The user id the folder names carry.
    pub(crate) uid: u32,
    /// `<topdir>/.Trash` is a real (not linked) folder with the sticky bit, and the user's
    /// `<uid>` folder below it exists and is the user's, or can be created.
    pub(crate) shared_usable: bool,
    pub(crate) own: OwnDir,
}

/// The Trash folder that was chosen. The folders `files/` and `info/` go below it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) enum TrashChoice {
    /// `$XDG_DATA_HOME/Trash`.
    Home(PathBuf),
    /// `<mount>/.Trash/<uid>`.
    Shared(PathBuf),
    /// `<mount>/.Trash-<uid>`.
    Own(PathBuf),
}

impl TrashChoice {
    /// The Trash folder itself.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn into_dir(self) -> PathBuf {
        match self {
            Self::Home(dir) | Self::Shared(dir) | Self::Own(dir) => dir,
        }
    }
}

/// The Trash folder to use, in the freedesktop order, or why there is none.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn choose_trash(facts: &TrashFacts) -> Result<TrashChoice, NoBin> {
    if facts.network {
        return Err(NoBin::Network);
    }
    if facts.read_only {
        return Err(NoBin::ReadOnly);
    }
    if let Some(home) = &facts.home {
        return Ok(TrashChoice::Home(home.clone()));
    }
    if facts.shared_usable {
        return Ok(TrashChoice::Shared(
            facts.topdir.join(".Trash").join(facts.uid.to_string()),
        ));
    }
    match facts.own {
        OwnDir::Usable | OwnDir::Creatable => Ok(TrashChoice::Own(
            facts.topdir.join(format!(".Trash-{}", facts.uid)),
        )),
        OwnDir::Unusable => Err(NoBin::NoTrashFolder),
    }
}

#[cfg(target_os = "linux")]
#[allow(unused_imports)]
pub(crate) use disk::{TrashEnv, trash_choice_with, trash_dir_for};

/// Reading the disk (Linux only).
#[cfg(target_os = "linux")]
mod disk {
    use std::fs::Metadata;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    use rustix::fs::{Access, access, major, minor};

    use super::super::bin_volumes::NoBin;
    use super::{
        OwnDir, TrashChoice, TrashFacts, choose_trash, entry_for, is_network_fs, parse_mountinfo,
    };

    /// The sticky bit of a folder's mode.
    const STICKY: u32 = 0o1000;

    /// What the choice reads from outside the path: the mounts, the home Trash and the user.
    /// A struct so tests can stand in for the real ones.
    #[derive(Debug, Clone)]
    pub(crate) struct TrashEnv {
        /// The text of `/proc/self/mountinfo`.
        pub(crate) mountinfo: String,
        /// `$XDG_DATA_HOME/Trash`, when the system has a data folder.
        pub(crate) home_trash: Option<PathBuf>,
        pub(crate) uid: u32,
    }

    impl TrashEnv {
        /// The real mounts, home Trash and user. Unreadable mounts give an empty list, which
        /// refuses every path.
        pub(crate) fn system() -> Self {
            Self {
                mountinfo: std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default(),
                home_trash: dirs::data_dir().map(|dir| dir.join("Trash")),
                uid: rustix::process::getuid().as_raw(),
            }
        }
    }

    /// The Trash folder that can keep `path` by a rename, or why none can.
    pub(crate) fn trash_dir_for(path: &Path) -> Result<PathBuf, NoBin> {
        trash_choice_with(path, &TrashEnv::system()).map(TrashChoice::into_dir)
    }

    /// Whether the user can write to a folder.
    fn writable(path: &Path) -> bool {
        access(path, Access::WRITE_OK).is_ok()
    }

    /// A real folder (not a link) on `dev`.
    fn real_dir_on(meta: &Metadata, dev: u64) -> bool {
        meta.file_type().is_dir() && meta.dev() == dev
    }

    /// The home Trash when it is on `dev` and the user can write to it (or, if it does not
    /// exist yet, to the nearest folder above it that does).
    fn usable_home(home: &Path, dev: u64) -> Option<PathBuf> {
        let nearest = home.ancestors().find(|a| std::fs::metadata(a).is_ok())?;
        let meta = std::fs::metadata(nearest).ok()?;
        (meta.is_dir() && meta.dev() == dev && writable(nearest)).then(|| home.to_owned())
    }

    /// `<topdir>/.Trash/<uid>` can be used.
    fn shared_usable(topdir: &Path, uid: u32, dev: u64) -> bool {
        let shared = topdir.join(".Trash");
        let Ok(meta) = std::fs::symlink_metadata(&shared) else {
            return false;
        };
        if !real_dir_on(&meta, dev) || meta.mode() & STICKY == 0 {
            return false;
        }
        let users = shared.join(uid.to_string());
        match std::fs::symlink_metadata(&users) {
            Ok(own) => real_dir_on(&own, dev) && own.uid() == uid && writable(&users),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => writable(&shared),
            Err(_) => false,
        }
    }

    /// The state of `<topdir>/.Trash-<uid>`.
    fn own_dir(topdir: &Path, uid: u32, dev: u64) -> OwnDir {
        let own = topdir.join(format!(".Trash-{uid}"));
        match std::fs::symlink_metadata(&own) {
            // On a FAT or exFAT drive there is no owner of its own: the drive reports the uid
            // it was mounted with, which is the user's when the user mounted it.
            Ok(meta) if real_dir_on(&meta, dev) && meta.uid() == uid && writable(&own) => {
                OwnDir::Usable
            }
            Ok(_) => OwnDir::Unusable,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && writable(topdir) => {
                OwnDir::Creatable
            }
            Err(_) => OwnDir::Unusable,
        }
    }

    /// [`trash_dir_for`] with the mounts, home Trash and user given. Every fact that cannot be
    /// read or does not fit refuses the path ("no Trash folder"): that sends it to the
    /// library folder's own trash folder, which is always safe.
    pub(crate) fn trash_choice_with(path: &Path, env: &TrashEnv) -> Result<TrashChoice, NoBin> {
        let refuse = NoBin::NoTrashFolder;
        if !path.is_absolute() {
            return Err(refuse);
        }
        // The path must exist (a dangling link does), and its folder is looked up without
        // links in the way because the mount list holds real paths.
        let own = std::fs::symlink_metadata(path).map_err(|_| refuse)?;
        path.file_name().ok_or(refuse)?;
        let parent = std::fs::canonicalize(path.parent().ok_or(refuse)?).map_err(|_| refuse)?;
        let dev = std::fs::metadata(&parent).map_err(|_| refuse)?.dev();
        // A path on another device than its folder is a mount point itself; it cannot be
        // renamed away.
        if own.dev() != dev {
            return Err(refuse);
        }

        let entries = parse_mountinfo(&env.mountinfo);
        let entry = entry_for(&parent.to_string_lossy(), &entries).ok_or(refuse)?;
        // The mount list and the folder must agree on the device; when they do not (a
        // sub-volume, an overlay), nothing is known about where a rename would land.
        if entry.dev != (major(dev), minor(dev)) {
            return Err(refuse);
        }
        let topdir = PathBuf::from(&entry.mount_point);
        // Another mount may hide the mount point; then it is not the root of this device.
        if std::fs::metadata(&topdir).map_err(|_| refuse)?.dev() != dev {
            return Err(refuse);
        }

        choose_trash(&TrashFacts {
            network: is_network_fs(&entry.fs_type),
            read_only: entry.read_only,
            home: env.home_trash.as_deref().and_then(|h| usable_home(h, dev)),
            shared_usable: shared_usable(&topdir, env.uid, dev),
            own: own_dir(&topdir, env.uid, dev),
            topdir,
            uid: env.uid,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = "\
22 28 0:21 / /sys rw,nosuid,nodev,noexec,relatime shared:7 - sysfs sysfs rw
28 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw,errors=remount-ro
40 28 8:17 / /media/alex/Films rw,nosuid,nodev,relatime shared:90 - exfat /dev/sdb1 rw,uid=1000
41 28 8:33 / /media/alex/My\\040Disk rw,relatime - ext4 /dev/sdc1 rw
50 28 0:45 / /mnt/nas rw,relatime shared:120 - nfs4 nas:/media rw,vers=4.2,proto=tcp
51 28 8:49 / /mnt/archive ro,relatime - ext4 /dev/sdd1 rw
52 28 8:65 / /mnt/superro rw,relatime - ext4 /dev/sde1 ro,errors=continue
garbage line
60 28 8:81 / /mnt/tab\\011and\\134slash rw - ext4 /dev/sdf1 rw
";

    #[test]
    fn mountinfo_is_parsed_with_escapes_types_and_read_only() {
        let entries = parse_mountinfo(MOUNTINFO);
        assert_eq!(entries.len(), 8, "the garbage line is skipped");
        let find = |mount: &str| entries.iter().find(|e| e.mount_point == mount).unwrap();
        assert_eq!(find("/").fs_type, "ext4");
        assert_eq!(find("/").dev, (8, 2));
        assert!(!find("/").read_only);
        // The optional field (`shared:90`) is skipped to reach the file system type.
        assert_eq!(find("/media/alex/Films").fs_type, "exfat");
        assert_eq!(
            find("/media/alex/My Disk").fs_type,
            "ext4",
            "\\040 is a space"
        );
        assert_eq!(find("/mnt/nas").fs_type, "nfs4");
        assert!(find("/mnt/archive").read_only, "ro among the mount options");
        assert!(
            find("/mnt/superro").read_only,
            "ro among the file system options"
        );
        assert!(
            entries
                .iter()
                .any(|e| e.mount_point == "/mnt/tab\tand\\slash"),
            "\\011 is a tab and \\134 a backslash"
        );
    }

    #[test]
    fn an_option_that_only_contains_ro_is_not_read_only() {
        let line =
            "9 1 8:2 / /data rw,errors=remount-ro,rootcontext - ext4 /dev/x rw,errors=remount-ro";
        assert!(!parse_mountinfo(line)[0].read_only);
        let rw = "9 1 8:2 / /data rw - ext4 /dev/x rw";
        assert!(!parse_mountinfo(rw)[0].read_only);
    }

    #[test]
    fn a_broken_escape_is_kept_as_it_is() {
        assert_eq!(unescape("a\\04b"), "a\\04b");
        assert_eq!(unescape("a\\999b"), "a\\999b", "not octal");
        assert_eq!(unescape("a\\777b"), "a\\777b", "above one byte");
        assert_eq!(unescape("trailing\\"), "trailing\\");
    }

    #[test]
    fn the_deepest_mount_holds_the_path() {
        let entries = parse_mountinfo(MOUNTINFO);
        let of = |path: &str| entry_for(path, &entries).map(|e| e.mount_point.as_str());
        assert_eq!(of("/media/alex/Films/Movies"), Some("/media/alex/Films"));
        assert_eq!(of("/media/alex/Films"), Some("/media/alex/Films"));
        assert_eq!(of("/media/alex/My Disk/TV/x"), Some("/media/alex/My Disk"));
        assert_eq!(of("/home/alex/Movies"), Some("/"));
        // A sibling that shares a prefix of letters is not a parent.
        assert_eq!(of("/media/alex/FilmsExtra/x"), Some("/"));
        assert_eq!(of("/mnt/nas/Movies"), Some("/mnt/nas"));
        // Case counts.
        assert_eq!(of("/media/alex/films/x"), Some("/"));
        assert_eq!(entry_for("/x", &[]), None);
    }

    #[test]
    fn a_mount_on_top_of_another_wins() {
        let text = "\
1 0 8:2 / / rw - ext4 /dev/a rw
2 1 8:3 / /data rw - ext4 /dev/b rw
3 1 0:50 / /data rw - nfs4 nas:/x rw
";
        let entries = parse_mountinfo(text);
        assert_eq!(entry_for("/data/x", &entries).unwrap().fs_type, "nfs4");
    }

    #[test]
    fn network_file_systems_are_named_exactly() {
        for kind in [
            "nfs",
            "nfs4",
            "cifs",
            "smb3",
            "smbfs",
            "9p",
            "afs",
            "ceph",
            "glusterfs",
            "fuse.sshfs",
            "fuse.rclone",
            "davfs",
            "fuse.davfs2",
            "fuse.gvfsd-fuse",
        ] {
            assert!(is_network_fs(kind), "{kind}");
        }
        for kind in [
            "ext4", "btrfs", "xfs", "exfat", "vfat", "ntfs3", "fuseblk", "tmpfs", "zfs",
        ] {
            assert!(!is_network_fs(kind), "{kind}");
        }
    }

    fn facts() -> TrashFacts {
        TrashFacts {
            network: false,
            read_only: false,
            home: None,
            topdir: PathBuf::from("/media/alex/Films"),
            uid: 1000,
            shared_usable: false,
            own: OwnDir::Unusable,
        }
    }

    #[test]
    fn the_home_trash_on_the_same_device_wins() {
        let facts = TrashFacts {
            home: Some(PathBuf::from("/home/alex/.local/share/Trash")),
            shared_usable: true,
            own: OwnDir::Usable,
            ..facts()
        };
        assert_eq!(
            choose_trash(&facts),
            Ok(TrashChoice::Home(PathBuf::from(
                "/home/alex/.local/share/Trash"
            )))
        );
    }

    #[test]
    fn a_sticky_shared_trash_with_the_users_folder_is_next() {
        let facts = TrashFacts {
            shared_usable: true,
            own: OwnDir::Usable,
            ..facts()
        };
        assert_eq!(
            choose_trash(&facts),
            Ok(TrashChoice::Shared(PathBuf::from(
                "/media/alex/Films/.Trash/1000"
            )))
        );
    }

    #[test]
    fn a_shared_trash_that_is_not_usable_is_ignored() {
        // Not sticky, a link or without the user's folder: `shared_usable` is false.
        for own in [OwnDir::Usable, OwnDir::Creatable] {
            let facts = TrashFacts { own, ..facts() };
            assert_eq!(
                choose_trash(&facts),
                Ok(TrashChoice::Own(PathBuf::from(
                    "/media/alex/Films/.Trash-1000"
                )))
            );
        }
    }

    #[test]
    fn nothing_usable_is_no_trash_folder() {
        // An existing but foreign `.Trash-<uid>` is never replaced.
        assert_eq!(choose_trash(&facts()), Err(NoBin::NoTrashFolder));
    }

    #[test]
    fn a_network_or_read_only_volume_is_refused_before_any_trash() {
        let usable = TrashFacts {
            home: Some(PathBuf::from("/h/Trash")),
            shared_usable: true,
            own: OwnDir::Usable,
            ..facts()
        };
        let network = TrashFacts {
            network: true,
            ..usable.clone()
        };
        assert_eq!(choose_trash(&network), Err(NoBin::Network));
        let read_only = TrashFacts {
            read_only: true,
            ..usable.clone()
        };
        assert_eq!(choose_trash(&read_only), Err(NoBin::ReadOnly));
        let both = TrashFacts {
            network: true,
            read_only: true,
            ..usable
        };
        assert_eq!(choose_trash(&both), Err(NoBin::Network));
    }

    /// Reading the real disk; run in Linux (WSL).
    #[cfg(target_os = "linux")]
    mod linux {
        use super::super::{TrashChoice, TrashEnv, trash_choice_with, trash_dir_for};
        use crate::library_ops::bin_volumes::NoBin;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        use std::path::{Path, PathBuf};

        /// A mountinfo line that makes a temp dir look like a mount of the given type, with
        /// the device the kernel reports for it.
        fn mount_line(dir: &Path, fs_type: &str, options: &str) -> String {
            let dev = std::fs::metadata(dir).unwrap().dev();
            format!(
                "100 1 {}:{} / {} {options} - {fs_type} test rw\n",
                rustix::fs::major(dev),
                rustix::fs::minor(dev),
                escaped(dir)
            )
        }

        /// A path the way `mountinfo` writes it: backslashes, spaces, tabs and newlines as
        /// octal escapes.
        fn escaped(path: &Path) -> String {
            path.to_string_lossy()
                .replace('\\', "\\134")
                .replace(' ', "\\040")
                .replace('\t', "\\011")
                .replace('\n', "\\012")
        }

        /// A temp dir on the Linux disk (the build folder may sit on a mount that keeps no
        /// owners or modes).
        fn scratch() -> tempfile::TempDir {
            tempfile::tempdir_in("/tmp").unwrap()
        }

        fn uid() -> u32 {
            rustix::process::getuid().as_raw()
        }

        fn env(mountinfo: String, home_trash: Option<PathBuf>) -> TrashEnv {
            TrashEnv {
                mountinfo,
                home_trash,
                uid: uid(),
            }
        }

        /// A temp dir as a pretend mount, with a movie file in it, and its mountinfo line.
        fn pretend_mount(fs_type: &str, options: &str) -> (tempfile::TempDir, PathBuf, String) {
            let dir = scratch();
            let file = dir.path().join("Heat.mkv");
            std::fs::write(&file, b"x").unwrap();
            let line = mount_line(dir.path(), fs_type, options);
            (dir, file, line)
        }

        fn chmod(path: &Path, mode: u32) {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }

        /// Root can write wherever a mode forbids it, so those cases cannot be shown.
        fn is_root() -> bool {
            rustix::process::getuid().is_root()
        }

        fn nearest_dev(path: &Path) -> u64 {
            path.ancestors()
                .find_map(|a| std::fs::metadata(a).ok())
                .unwrap()
                .dev()
        }

        #[test]
        fn a_temp_dir_under_home_resolves_to_the_home_trash() {
            let (Some(home), Some(data)) = (dirs::home_dir(), dirs::data_dir()) else {
                return;
            };
            let Ok(dir) = tempfile::tempdir_in(&home) else {
                return;
            };
            let file = dir.path().join("Heat.mkv");
            std::fs::write(&file, b"x").unwrap();
            if std::fs::metadata(dir.path()).unwrap().dev() != nearest_dev(&data.join("Trash")) {
                eprintln!("skipped: the data folder is not on the disk of $HOME");
                return;
            }
            // The real mount list may not describe a build in a container; then it refuses.
            match trash_dir_for(&file) {
                Ok(dir) => assert_eq!(dir, data.join("Trash")),
                Err(why) => eprintln!("skipped: the mount list refused the path: {why:?}"),
            }
        }

        #[test]
        fn the_home_trash_on_the_same_device_is_chosen_and_nothing_is_created() {
            let (dir, file, line) = pretend_mount("ext4", "rw");
            let home = dir.path().join("share/Trash");
            let got = trash_choice_with(&file, &env(line, Some(home.clone())));
            assert_eq!(got, Ok(TrashChoice::Home(home.clone())));
            assert!(!home.exists(), "choosing creates no folder");
            assert!(!dir.path().join(".Trash").exists());
            assert!(!dir.path().join(format!(".Trash-{}", uid())).exists());
        }

        #[test]
        fn an_unwritable_home_trash_is_not_used() {
            if is_root() {
                return;
            }
            let (dir, file, line) = pretend_mount("ext4", "rw");
            let share = dir.path().join("share");
            std::fs::create_dir(&share).unwrap();
            chmod(&share, 0o555);
            let got = trash_choice_with(&file, &env(line, Some(share.join("Trash"))));
            chmod(&share, 0o755);
            // The mount point (the temp dir) is writable, so the drive's own folder is next.
            assert_eq!(
                got,
                Ok(TrashChoice::Own(
                    dir.path().join(format!(".Trash-{}", uid()))
                ))
            );
        }

        #[test]
        fn a_home_trash_on_another_device_is_not_used() {
            let shm = Path::new("/dev/shm");
            if !shm.is_dir() {
                return;
            }
            let (dir, file, line) = pretend_mount("ext4", "rw");
            let Ok(other) = tempfile::tempdir_in(shm) else {
                return;
            };
            if std::fs::metadata(other.path()).unwrap().dev()
                == std::fs::metadata(dir.path()).unwrap().dev()
            {
                eprintln!("skipped: /dev/shm is on the same device");
                return;
            }
            let got = trash_choice_with(&file, &env(line, Some(other.path().join("Trash"))));
            assert_eq!(
                got,
                Ok(TrashChoice::Own(
                    dir.path().join(format!(".Trash-{}", uid()))
                ))
            );
        }

        #[test]
        fn a_file_on_a_second_file_system_gets_that_drives_own_trash_folder() {
            let shm = Path::new("/dev/shm");
            let Ok(dir) = tempfile::tempdir_in(shm) else {
                return;
            };
            let file = dir.path().join("Heat.mkv");
            std::fs::write(&file, b"x").unwrap();
            // The real mount list, and a home Trash on the disk of the build.
            let home = scratch();
            if std::fs::metadata(home.path()).unwrap().dev()
                == std::fs::metadata(dir.path()).unwrap().dev()
            {
                eprintln!("skipped: /dev/shm is on the same device as the temp folder");
                return;
            }
            let Ok(mounts) = std::fs::read_to_string("/proc/self/mountinfo") else {
                return;
            };
            let got = trash_choice_with(&file, &env(mounts, Some(home.path().join("Trash"))));
            // The mount list of a container may not name /dev/shm; then the path is refused.
            // Never the home Trash of another device.
            match got {
                Ok(TrashChoice::Own(own)) => {
                    assert_eq!(own, PathBuf::from(format!("/dev/shm/.Trash-{}", uid())));
                    assert!(!own.exists(), "choosing creates no folder");
                }
                Err(NoBin::NoTrashFolder) => {}
                other => panic!("unexpected: {other:?}"),
            }
        }

        #[test]
        fn a_sticky_shared_trash_is_used_before_the_own_folder() {
            let (dir, file, line) = pretend_mount("ext4", "rw");
            let shared = dir.path().join(".Trash");
            std::fs::create_dir(&shared).unwrap();
            chmod(&shared, 0o1777);
            let mine = shared.join(uid().to_string());
            // The user's folder below it does not exist yet: it can be created.
            assert_eq!(
                trash_choice_with(&file, &env(line.clone(), None)),
                Ok(TrashChoice::Shared(mine.clone()))
            );
            // Present and the user's: still used.
            std::fs::create_dir(&mine).unwrap();
            assert_eq!(
                trash_choice_with(&file, &env(line, None)),
                Ok(TrashChoice::Shared(mine))
            );
        }

        #[test]
        fn a_shared_trash_without_the_sticky_bit_is_ignored() {
            let (dir, file, line) = pretend_mount("ext4", "rw");
            let shared = dir.path().join(".Trash");
            std::fs::create_dir(&shared).unwrap();
            chmod(&shared, 0o777);
            assert_eq!(
                trash_choice_with(&file, &env(line, None)),
                Ok(TrashChoice::Own(
                    dir.path().join(format!(".Trash-{}", uid()))
                ))
            );
        }

        #[test]
        fn a_shared_trash_that_is_a_link_is_ignored() {
            let (dir, file, line) = pretend_mount("ext4", "rw");
            let real = dir.path().join("elsewhere");
            std::fs::create_dir(&real).unwrap();
            chmod(&real, 0o1777);
            std::os::unix::fs::symlink(&real, dir.path().join(".Trash")).unwrap();
            assert_eq!(
                trash_choice_with(&file, &env(line, None)),
                Ok(TrashChoice::Own(
                    dir.path().join(format!(".Trash-{}", uid()))
                ))
            );
        }

        #[test]
        fn a_shared_trash_the_user_cannot_add_a_folder_to_is_ignored() {
            if is_root() {
                return;
            }
            let (dir, file, line) = pretend_mount("ext4", "rw");
            let shared = dir.path().join(".Trash");
            std::fs::create_dir(&shared).unwrap();
            // Sticky, but nobody may add the user's folder.
            chmod(&shared, 0o1555);
            let got = trash_choice_with(&file, &env(line, None));
            chmod(&shared, 0o1777);
            assert_eq!(
                got,
                Ok(TrashChoice::Own(
                    dir.path().join(format!(".Trash-{}", uid()))
                ))
            );
        }

        #[test]
        fn a_users_folder_in_the_shared_trash_that_is_a_link_is_ignored() {
            let (dir, file, line) = pretend_mount("ext4", "rw");
            let shared = dir.path().join(".Trash");
            std::fs::create_dir(&shared).unwrap();
            chmod(&shared, 0o1777);
            let elsewhere = dir.path().join("elsewhere");
            std::fs::create_dir(&elsewhere).unwrap();
            std::os::unix::fs::symlink(&elsewhere, shared.join(uid().to_string())).unwrap();
            assert_eq!(
                trash_choice_with(&file, &env(line, None)),
                Ok(TrashChoice::Own(
                    dir.path().join(format!(".Trash-{}", uid()))
                ))
            );
        }

        #[test]
        fn a_numbered_folder_of_another_owner_is_never_taken_over() {
            // The environment claims a user id the folders do not belong to.
            let (dir, file, line) = pretend_mount("ext4", "rw");
            let pretend = uid() + 1;
            std::fs::create_dir(dir.path().join(format!(".Trash-{pretend}"))).unwrap();
            let mut other = env(line, None);
            other.uid = pretend;
            // `.Trash-<pretend>` exists but is owned by the real user, not by `pretend`.
            assert_eq!(trash_choice_with(&file, &other), Err(NoBin::NoTrashFolder));
        }

        #[test]
        fn an_own_folder_that_exists_is_used_and_a_link_or_file_is_not() {
            let (dir, file, line) = pretend_mount("ext4", "rw");
            let own = dir.path().join(format!(".Trash-{}", uid()));
            std::fs::create_dir(&own).unwrap();
            assert_eq!(
                trash_choice_with(&file, &env(line.clone(), None)),
                Ok(TrashChoice::Own(own.clone()))
            );
            std::fs::remove_dir(&own).unwrap();
            let target = dir.path().join("other");
            std::fs::create_dir(&target).unwrap();
            std::os::unix::fs::symlink(&target, &own).unwrap();
            assert_eq!(
                trash_choice_with(&file, &env(line.clone(), None)),
                Err(NoBin::NoTrashFolder),
                "a link is not replaced and not followed"
            );
            std::fs::remove_file(&own).unwrap();
            std::fs::write(&own, b"a file").unwrap();
            assert_eq!(
                trash_choice_with(&file, &env(line, None)),
                Err(NoBin::NoTrashFolder)
            );
        }

        #[test]
        fn a_mount_point_that_is_not_writable_has_no_trash_folder() {
            if is_root() {
                return;
            }
            let (dir, file, line) = pretend_mount("ext4", "rw");
            chmod(dir.path(), 0o555);
            let got = trash_choice_with(&file, &env(line, None));
            chmod(dir.path(), 0o755);
            assert_eq!(got, Err(NoBin::NoTrashFolder));
        }

        #[test]
        fn a_network_file_system_is_refused_whatever_else_is_there() {
            for fs_type in ["nfs4", "cifs", "fuse.sshfs", "9p"] {
                let (dir, file, line) = pretend_mount(fs_type, "rw");
                let home = dir.path().join("Trash");
                assert_eq!(
                    trash_choice_with(&file, &env(line, Some(home))),
                    Err(NoBin::Network),
                    "{fs_type}"
                );
            }
        }

        #[test]
        fn a_read_only_mount_is_refused() {
            let (dir, file, line) = pretend_mount("ext4", "ro,relatime");
            let home = dir.path().join("Trash");
            assert_eq!(
                trash_choice_with(&file, &env(line, Some(home))),
                Err(NoBin::ReadOnly)
            );
        }

        #[test]
        fn a_mount_list_that_names_another_device_refuses_the_path() {
            let (dir, file, _) = pretend_mount("ext4", "rw");
            let dev = std::fs::metadata(dir.path()).unwrap().dev();
            let wrong = format!(
                "100 1 {}:{} / {} rw - ext4 test rw\n",
                rustix::fs::major(dev) + 1,
                rustix::fs::minor(dev),
                escaped(dir.path())
            );
            assert_eq!(
                trash_choice_with(&file, &env(wrong, Some(dir.path().join("Trash")))),
                Err(NoBin::NoTrashFolder)
            );
        }

        #[test]
        fn an_unreadable_mount_list_refuses_the_path() {
            let (dir, file, _) = pretend_mount("ext4", "rw");
            assert_eq!(
                trash_choice_with(&file, &env(String::new(), Some(dir.path().join("Trash")))),
                Err(NoBin::NoTrashFolder)
            );
        }

        #[test]
        fn a_missing_or_relative_path_is_refused() {
            let (dir, _, line) = pretend_mount("ext4", "rw");
            let e = env(line, Some(dir.path().join("Trash")));
            assert_eq!(
                trash_choice_with(&dir.path().join("nothing.mkv"), &e),
                Err(NoBin::NoTrashFolder)
            );
            assert_eq!(
                trash_choice_with(Path::new("Heat.mkv"), &e),
                Err(NoBin::NoTrashFolder)
            );
            assert_eq!(
                trash_choice_with(Path::new("/"), &e),
                Err(NoBin::NoTrashFolder)
            );
        }

        #[test]
        fn a_folder_and_a_dangling_link_are_judged_like_a_file() {
            let (dir, _, line) = pretend_mount("ext4", "rw");
            let home = dir.path().join("Trash");
            let e = env(line, Some(home.clone()));
            let folder = dir.path().join("Heat (1995)");
            std::fs::create_dir(&folder).unwrap();
            assert_eq!(
                trash_choice_with(&folder, &e),
                Ok(TrashChoice::Home(home.clone()))
            );
            let link = dir.path().join("dangling");
            std::os::unix::fs::symlink(dir.path().join("gone"), &link).unwrap();
            assert_eq!(trash_choice_with(&link, &e), Ok(TrashChoice::Home(home)));
        }

        #[test]
        fn a_path_reached_through_a_link_is_looked_up_by_its_real_folder() {
            let (dir, _, line) = pretend_mount("nfs4", "rw");
            let real = dir.path().join("real");
            std::fs::create_dir(&real).unwrap();
            std::fs::write(real.join("a.mkv"), b"x").unwrap();
            let via = dir.path().join("via");
            std::os::unix::fs::symlink(&real, &via).unwrap();
            // The mount list names the real path, so the link is resolved to find the mount.
            let got = trash_choice_with(&via.join("a.mkv"), &env(line, None));
            assert_eq!(got, Err(NoBin::Network));
        }

        #[test]
        fn a_mount_point_itself_is_refused() {
            let shm = Path::new("/dev/shm");
            let Ok(shm_meta) = std::fs::metadata(shm) else {
                return;
            };
            if shm_meta.dev() == std::fs::metadata("/dev").unwrap().dev() {
                return;
            }
            let Ok(mounts) = std::fs::read_to_string("/proc/self/mountinfo") else {
                return;
            };
            // `/dev/shm` is on another device than `/dev`: it cannot be renamed into a Trash.
            let e = env(mounts, Some(PathBuf::from("/dev/shm/none")));
            assert_eq!(trash_choice_with(shm, &e), Err(NoBin::NoTrashFolder));
        }
    }
}
