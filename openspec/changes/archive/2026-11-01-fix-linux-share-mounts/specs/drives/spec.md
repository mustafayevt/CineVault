## MODIFIED Requirements

### Requirement: Which drive a folder is on
A library folder SHALL belong to the volume that holds it: a Windows drive letter, a network share, a macOS mount point or a Linux mount point (such as `/media/<user>/<name>`, `/run/media/<user>/<name>`, `/mnt/<name>` or `/`). On Linux the mount point of a folder below `/media`, `/run/media` or `/mnt` SHALL be the deepest folder from `/media/<name>`, `/run/media/<user>` or `/mnt/<name>` down to `/media/<user>/<name>`, `/run/media/<user>/<name>` or `/mnt/<name>` that the system's mount list names, and that usual mount point when the list names none of them. Two folders on one volume SHALL be one drive with two folders. A Linux drive with no volume label SHALL be named after its mount point's last part. A drive SHALL be online when at least one of its library folders answers the health check, and every folder that answers SHALL record when it was last seen. On Linux a library folder SHALL NOT answer while its mount point exists but is not mounted (the drive is unmounted).

#### Scenario: Two folders on one drive
- **WHEN** `E:\Movies` and `E:\TV` are library folders
- **THEN** one drive E: is listed with both folders

#### Scenario: Two folders on one Linux disk
- **WHEN** `/media/alex/Films/Movies` and `/media/alex/Films/TV` are library folders on Linux
- **THEN** one drive "Films" is listed with both folders

#### Scenario: A share mounted higher up
- **WHEN** a network share is mounted at `/media/nas`, `/media/nas` is a library folder and it holds the folder `/media/nas/Example Movie (2001)` with its video file
- **THEN** the title is online, scans keep it online, and Rename and Move offer it

#### Scenario: Unmounted Linux disk
- **WHEN** the disk mounted at `/mnt/films` is unmounted, leaving the empty folder `/mnt/films`, and `/mnt/films` is a library folder
- **THEN** the drive is offline, its titles keep their state, and a scan does not mark them missing

### Requirement: Recovery waits for an unmounted Linux drive
On Linux, recovering an interrupted rename or move batch SHALL treat a drive as reachable only when its mount point (as found under "Which drive a folder is on", or `/` for other paths) is mounted. A batch on an unmounted drive SHALL wait until the drive is back, as for a disconnected drive on Windows.

#### Scenario: Batch on an unmounted disk
- **WHEN** CineVault starts after a rename batch on `/media/alex/Films` was interrupted and that disk is not mounted
- **THEN** the batch is left waiting, nothing is written below `/media/alex/Films`, and it is settled once the disk is mounted again
