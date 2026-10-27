//! Whether the system bin can take a path (safety review finding 4).
//!
//! On Windows only a fixed local drive has a Recycle Bin. A network share, a mapped network
//! drive, a removable drive (a USB stick) and a drive whose bin is turned off have none, and an
//! item larger than the bin's maximum size does not fit: the shell would delete such a path for
//! good. [`bin_refusal`] says so before anything is sent, and the caller moves the path to the
//! library folder's trash folder instead.
//!
//! The drive type comes from the volumes `sysinfo` lists (it asks `GetDriveTypeW` and lists only
//! fixed and removable volumes, so a mapped network drive is not among them); the bin's size and
//! switch come from `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume\
//! {GUID}` (`MaxCapacity` in MB, `NukeOnDelete`), the GUID of a volume from `mountvol <mount> /L`
//! (`HKLM\SYSTEM\MountedDevices` does not list every volume). A setting that cannot be read is no limit; the check after the
//! send still notices an item that did not reach the bin.
//!
//! macOS keeps its behaviour: every volume is taken to have a Trash (see the manual checks).
//!
//! On Linux a volume has a Trash only when the freedesktop Trash can keep an item there by one
//! rename on the same file system (`xdg_dirs`): not on a network or read-only file system, and
//! only when a Trash folder on the same device can be used. Whenever that is unclear the
//! answer is no, and the item goes to the library folder's trash folder instead; it is never
//! copied into a Trash on another drive.

use std::path::Path;

/// Why a volume has no system bin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoBin {
    /// A network share or a mapped network drive.
    Network,
    /// A removable drive, such as a USB stick or a memory card.
    Removable,
    /// The bin is turned off for this drive ("Don't move files to the Recycle Bin").
    TurnedOff,
    /// Not a local fixed drive the system lists (a mapped or substituted drive, a disc).
    NotLocal,
    /// Linux: no Trash folder on this drive's file system can be used, so an item could not be
    /// kept there by a rename (and is never copied into another drive's Trash).
    NoTrashFolder,
    /// Linux: the file system is mounted read-only.
    ReadOnly,
}

/// What the system bin can take on the volume of a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeBin {
    /// The volume has a bin. `max_bytes` is the largest item it keeps, when the system says.
    Takes { max_bytes: Option<u64> },
    /// The volume has no bin.
    None(NoBin),
}

/// Says what the system bin can take where a path lives. A trait so tests never read the real
/// drives or registry.
pub trait BinVolumes: Send + Sync {
    fn bin_of(&self, path: &Path) -> VolumeBin;
}

/// Why the system bin cannot take `path`, in words; `None` when it can. `size` gives the bytes
/// of the path (a folder summed) and is asked only when the bin has a size limit.
pub fn bin_refusal(
    volumes: &dyn BinVolumes,
    path: &Path,
    size: impl FnOnce(&Path) -> u64,
) -> Option<String> {
    match volumes.bin_of(path) {
        VolumeBin::None(why) => Some(
            match why {
                NoBin::Network if cfg!(windows) => {
                    "This is a network drive, which has no Recycle Bin"
                }
                NoBin::Network => "This is a network drive, which has no Trash",
                NoBin::Removable => "This is a removable drive, which has no Recycle Bin",
                NoBin::TurnedOff => "The Recycle Bin is turned off for this drive",
                NoBin::NotLocal => "This drive has no Recycle Bin",
                NoBin::NoTrashFolder => "This drive has no Trash folder CineVault can use",
                NoBin::ReadOnly => "This drive is read-only",
            }
            .to_owned(),
        ),
        VolumeBin::Takes {
            max_bytes: Some(max),
        } if size(path) > max => Some(format!(
            "{} is larger than the Recycle Bin of its drive can hold",
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned()
            )
        )),
        VolumeBin::Takes { .. } => None,
    }
}

/// The real drives.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemVolumes;

#[cfg(target_os = "macos")]
impl BinVolumes for SystemVolumes {
    fn bin_of(&self, _path: &Path) -> VolumeBin {
        VolumeBin::Takes { max_bytes: None }
    }
}

#[cfg(target_os = "linux")]
impl BinVolumes for SystemVolumes {
    fn bin_of(&self, path: &Path) -> VolumeBin {
        // The freedesktop Trash has no size limit CineVault can read; the check after the
        // send still reports an item that left its place but is not in the Trash.
        match super::xdg_dirs::trash_dir_for(path) {
            Ok(_) => VolumeBin::Takes { max_bytes: None },
            Err(why) => VolumeBin::None(why),
        }
    }
}

#[cfg(windows)]
impl BinVolumes for SystemVolumes {
    fn bin_of(&self, path: &Path) -> VolumeBin {
        let text = path.to_string_lossy();
        if is_network_path(&text) {
            return VolumeBin::None(NoBin::Network);
        }
        let volumes = windows::volumes();
        let Some(volume) = pick_volume(&text, &volumes) else {
            return VolumeBin::None(NoBin::NotLocal);
        };
        if volume.removable {
            return VolumeBin::None(NoBin::Removable);
        }
        volume
            .guid
            .as_deref()
            .map_or(VolumeBin::Takes { max_bytes: None }, windows::bin_settings)
    }
}

/// A local volume the system lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalVolume {
    /// Where it is mounted, upper case, ending in `\` (`D:\`, `C:\Mounts\Media\`).
    pub(crate) mount: String,
    pub(crate) removable: bool,
    /// `{xxxxxxxx-...}`, when known.
    pub(crate) guid: Option<String>,
}

/// A path written the plain Windows way: `\` separators, without the `\\?\` prefix (a
/// `\\?\UNC\` path becomes `\\server\share`), upper case.
#[cfg_attr(not(windows), allow(dead_code))]
fn plain(path: &str) -> String {
    let unified = path.replace('/', "\\");
    let unified = match unified.strip_prefix("\\\\?\\") {
        Some(rest) => match rest.strip_prefix("UNC\\") {
            Some(share) => format!("\\\\{share}"),
            None => rest.to_owned(),
        },
        None => unified,
    };
    unified.to_uppercase()
}

/// A Windows network path (`\\server\share\...`, `\\?\UNC\...`; not `\\?\C:\...`).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn is_network_path(path: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    plain(path).starts_with("\\\\")
}

/// The volume `path` is on: the one with the longest mount point above it.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn pick_volume<'a>(path: &str, volumes: &'a [LocalVolume]) -> Option<&'a LocalVolume> {
    let mut path = plain(path);
    if !path.ends_with('\\') {
        path.push('\\');
    }
    volumes
        .iter()
        .filter(|v| path.starts_with(&v.mount))
        .max_by_key(|v| v.mount.len())
}

/// The volume GUID (`{...}`) in what `mountvol <mount> /L` prints: `\\?\Volume{...}\`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn guid_in_mountvol(output: &str) -> Option<String> {
    let start = output.find("Volume{")? + "Volume".len();
    let rest = &output[start..];
    let end = rest.find('}')?;
    Some(rest[..=end].to_owned())
}

/// The bin of a volume from its `NukeOnDelete` and `MaxCapacity` (MB) settings.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn bin_from(nuke_on_delete: Option<u32>, max_capacity_mb: Option<u32>) -> VolumeBin {
    if nuke_on_delete == Some(1) || max_capacity_mb == Some(0) {
        return VolumeBin::None(NoBin::TurnedOff);
    }
    VolumeBin::Takes {
        max_bytes: max_capacity_mb.map(|mb| u64::from(mb) * 1024 * 1024),
    }
}

#[cfg(windows)]
mod windows {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    use sysinfo::{DiskRefreshKind, Disks};
    use winreg::HKCU;

    use super::{LocalVolume, VolumeBin, bin_from, guid_in_mountvol};

    /// Starts a console program without opening a window.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// How long the list of volumes is reused: a delete or a move asks once per path.
    const FRESH: Duration = Duration::from_secs(10);

    static CACHE: Mutex<Option<(Instant, Vec<LocalVolume>)>> = Mutex::new(None);

    /// The fixed and removable volumes, with their GUIDs.
    pub(super) fn volumes() -> Vec<LocalVolume> {
        let mut cache = CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((at, list)) = cache.as_ref()
            && at.elapsed() < FRESH
        {
            return list.clone();
        }
        let disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing());
        let list: Vec<LocalVolume> = disks
            .list()
            .iter()
            .map(|disk| {
                let mut mount = disk.mount_point().to_string_lossy().to_uppercase();
                if !mount.ends_with('\\') {
                    mount.push('\\');
                }
                let removable = disk.is_removable();
                LocalVolume {
                    // A removable drive has no bin whatever its settings say.
                    guid: if removable { None } else { guid_of(&mount) },
                    removable,
                    mount,
                }
            })
            .collect();
        *cache = Some((Instant::now(), list.clone()));
        list
    }

    /// The volume GUID of a mount point, from `mountvol` (no window opens).
    fn guid_of(mount: &str) -> Option<String> {
        let system = std::env::var_os("SystemRoot").map_or_else(
            || std::path::PathBuf::from(r"C:\Windows"),
            std::path::PathBuf::from,
        );
        let output = Command::new(system.join("System32").join("mountvol.exe"))
            .arg(mount)
            .arg("/L")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        guid_in_mountvol(&String::from_utf8_lossy(&output.stdout))
    }

    /// The Recycle Bin settings of the volume `guid`.
    pub(super) fn bin_settings(guid: &str) -> VolumeBin {
        let path = format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\BitBucket\\Volume\\{guid}"
        );
        let Ok(key) = HKCU.open_subkey(path) else {
            return VolumeBin::Takes { max_bytes: None };
        };
        bin_from(
            key.get_value::<u32, _>("NukeOnDelete").ok(),
            key.get_value::<u32, _>("MaxCapacity").ok(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(VolumeBin);

    impl BinVolumes for Fixed {
        fn bin_of(&self, _path: &Path) -> VolumeBin {
            self.0
        }
    }

    #[test]
    fn a_fixed_drive_with_room_takes_the_path() {
        let bin = Fixed(VolumeBin::Takes {
            max_bytes: Some(1000),
        });
        assert_eq!(bin_refusal(&bin, Path::new("D:/Heat"), |_| 999), None);
        let open = Fixed(VolumeBin::Takes { max_bytes: None });
        assert_eq!(bin_refusal(&open, Path::new("D:/Heat"), |_| u64::MAX), None);
    }

    #[test]
    fn the_network_reason_names_the_bin_of_the_system() {
        let said = bin_refusal(
            &Fixed(VolumeBin::None(NoBin::Network)),
            Path::new("/mnt/nas/Heat"),
            |_| 0,
        )
        .unwrap();
        let bin = if cfg!(windows) {
            "Recycle Bin"
        } else {
            "Trash"
        };
        assert_eq!(said, format!("This is a network drive, which has no {bin}"));
    }

    #[test]
    fn an_item_larger_than_the_bin_is_refused() {
        let bin = Fixed(VolumeBin::Takes {
            max_bytes: Some(1000),
        });
        let why = bin_refusal(&bin, Path::new("D:/Movies/Heat"), |_| 1001).unwrap();
        assert!(why.contains("Heat is larger"), "{why}");
    }

    #[test]
    fn drives_without_a_bin_are_refused_without_sizing() {
        for (why, text) in [
            (NoBin::Network, "network drive"),
            (NoBin::Removable, "removable drive"),
            (NoBin::TurnedOff, "turned off"),
            (NoBin::NotLocal, "no Recycle Bin"),
            (NoBin::NoTrashFolder, "no Trash folder CineVault can use"),
            (NoBin::ReadOnly, "read-only"),
        ] {
            let said = bin_refusal(&Fixed(VolumeBin::None(why)), Path::new("Z:/Heat"), |_| {
                panic!("a drive without a bin needs no size")
            })
            .unwrap();
            assert!(said.contains(text), "{said}");
        }
    }

    fn vol(mount: &str, removable: bool) -> LocalVolume {
        LocalVolume {
            mount: mount.to_owned(),
            removable,
            guid: None,
        }
    }

    #[test]
    fn the_volume_of_a_path_is_the_deepest_mount_above_it() {
        let list = [
            vol("C:\\", false),
            vol("C:\\MOUNTS\\MEDIA\\", true),
            vol("E:\\", true),
        ];
        assert_eq!(pick_volume(r"c:\Movies\Heat", &list).unwrap().mount, "C:\\");
        assert_eq!(
            pick_volume(r"\\?\C:\Mounts\Media\Heat", &list)
                .unwrap()
                .mount,
            "C:\\MOUNTS\\MEDIA\\"
        );
        assert!(pick_volume("E:/Heat", &list).unwrap().removable);
        assert_eq!(
            pick_volume(r"Z:\Movies\Heat", &list),
            None,
            "a mapped drive"
        );
        assert_eq!(pick_volume(r"\\nas\media\Heat", &list), None);
        assert_eq!(pick_volume(r"C:", &list).unwrap().mount, "C:\\");
    }

    #[test]
    fn the_guid_of_a_volume_is_read_from_mountvol() {
        assert_eq!(
            guid_in_mountvol("    \\\\?\\Volume{78b56cee-4fa4-4985-8a0f-7737fc4e048d}\\\r\n")
                .as_deref(),
            Some("{78b56cee-4fa4-4985-8a0f-7737fc4e048d}")
        );
        assert_eq!(guid_in_mountvol("The parameter is incorrect.\r\n"), None);
    }

    #[test]
    fn bin_settings_turn_into_a_limit_or_no_bin() {
        assert_eq!(
            bin_from(Some(1), Some(5000)),
            VolumeBin::None(NoBin::TurnedOff)
        );
        assert_eq!(bin_from(None, Some(0)), VolumeBin::None(NoBin::TurnedOff));
        assert_eq!(
            bin_from(Some(0), Some(2)),
            VolumeBin::Takes {
                max_bytes: Some(2 * 1024 * 1024)
            }
        );
        assert_eq!(bin_from(None, None), VolumeBin::Takes { max_bytes: None });
    }

    #[test]
    #[cfg(windows)]
    fn network_paths_have_no_bin() {
        assert!(is_network_path(r"\\nas\media\Movies\Heat"));
        assert!(is_network_path(r"\\?\UNC\nas\media\Heat"));
        assert!(!is_network_path(r"\\?\C:\Movies\Heat"));
        assert!(!is_network_path(r"D:\Movies\Heat"));
        assert_eq!(
            SystemVolumes.bin_of(Path::new(r"\\nas\media\Heat")),
            VolumeBin::None(NoBin::Network)
        );
    }

    /// The real drives, opt-in (reads only): `CINEVAULT_LIVE_BIN=1 cargo test -p cv-app
    /// bin_live`. The build folder must be on a fixed drive with its Recycle Bin on.
    #[test]
    #[cfg(windows)]
    fn bin_live_the_build_folder_drive_has_a_bin() {
        if std::env::var("CINEVAULT_LIVE_BIN").as_deref() != Ok("1") {
            eprintln!("skipped: set CINEVAULT_LIVE_BIN=1 to read the real drives");
            return;
        }
        let here = std::env::current_dir().unwrap();
        let bin = SystemVolumes.bin_of(&here);
        eprintln!("{} -> {bin:?}", here.display());
        assert!(
            matches!(bin, VolumeBin::Takes { max_bytes: Some(_) }),
            "the bin size of a fixed drive is read: {bin:?}"
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn only_windows_network_paths_are_refused() {
        assert!(!is_network_path("//nas/media/Heat"));
    }
}
