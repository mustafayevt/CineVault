## Why

On Linux, CineVault tells an unplugged drive from a mounted one by the folder the drive is mounted on, and it found that folder from the path alone: `/media/<user>/<name>`, `/run/media/<user>/<name>` or `/mnt/<name>`. A network share mounted one level higher, at `/media/nas`, does not fit that shape, and neither does a folder a container binds at `/media/movies`. For a title in `/media/nas/Heat (1995)` CineVault took `/media/nas/Heat (1995)` for the drive, saw it was not a mount point, and stored every title of the library as Missing, so Rename refused them all (issue #28). Every browser-mode container install hits the same rule, because the installer binds each library folder at `/media/<name>`.

## What Changes

- On Linux the drive that holds a folder is the deepest folder between `/media/<name>` (or `/run/media/<user>`, `/mnt/<name>`) and the usual mount point that the system's mount list names. Only when the mount list names none of them is the usual mount point taken from the path, as before.
- The titles on a share mounted at `/media/nas` or in a container's bind at `/media/movies` are online, a scan and the next start keep them so, and Rename and Move work on them.
- An unplugged drive still reads as offline, never as missing: when nothing is mounted at `/media/<user>/<name>`, its leftover folder still does not answer.

Out of scope: how a library folder on a network share is added (an SMB option with a user name and password is issue #25), Windows and macOS (they never used this rule), and the health check's timing.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `drives`: "Which drive a folder is on" and "Recovery waits for an unmounted Linux drive" (the mount list decides the Linux mount point before the path does).

## Impact

`crates/cv-fs/src/paths.rs` (`linux_mount_root` reads the mount list; the pure `linux_mount_root_with` is tested on every platform) and two comments that described the old rule in `cv-app`. Every caller of the rule follows: the availability check, the scan's health check, History's place names, rename recovery, and the Drives page for a folder the system's disk list does not show. No database, IPC, UI or file-format change.
