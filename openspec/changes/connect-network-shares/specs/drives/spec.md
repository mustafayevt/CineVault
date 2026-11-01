## MODIFIED Requirements

### Requirement: Which drive a folder is on
A library folder SHALL belong to the volume that holds it: a Windows drive letter, a network share, a macOS mount point or a Linux mount point (such as `/media/<user>/<name>`, `/run/media/<user>/<name>`, `/mnt/<name>` or `/`). A folder on a share connected by CineVault SHALL belong to that share, named like `\\nas\movies`, wherever the share is mounted. Two folders on one volume SHALL be one drive with two folders. A Linux drive with no volume label SHALL be named after its mount point's last part. A drive SHALL be online when at least one of its library folders answers the health check, and every folder that answers SHALL record when it was last seen. On Linux a library folder below `/media/<user>/<name>`, `/run/media/<user>/<name>` or `/mnt/<name>` SHALL NOT answer while that folder exists but is not a mount point (the drive is unmounted).

#### Scenario: Two folders on one drive
- **WHEN** `E:\Movies` and `E:\TV` are library folders
- **THEN** one drive E: is listed with both folders

#### Scenario: Two folders on one Linux disk
- **WHEN** `/media/alex/Films/Movies` and `/media/alex/Films/TV` are library folders on Linux
- **THEN** one drive "Films" is listed with both folders

#### Scenario: Unmounted Linux disk
- **WHEN** the disk mounted at `/mnt/films` is unmounted, leaving the empty folder `/mnt/films`, and `/mnt/films` is a library folder
- **THEN** the drive is offline, its titles keep their state, and a scan does not mark them missing

#### Scenario: Share mounted by CineVault on macOS
- **WHEN** CineVault mounts the share `movies` of the server `nas` on macOS and its folders `Films` and `Series` are library folders
- **THEN** one drive `\\nas\movies` is listed with both folders
