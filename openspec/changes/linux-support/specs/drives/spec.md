## MODIFIED Requirements

### Requirement: Which drive a folder is on
A library folder SHALL belong to the volume that holds it: a Windows drive letter, a network share, a macOS mount point or a Linux mount point (such as `/media/<user>/<name>`, `/run/media/<user>/<name>`, `/mnt/<name>` or `/`). Two folders on one volume SHALL be one drive with two folders. A Linux drive with no volume label SHALL be named after its mount point's last part. A drive SHALL be online when at least one of its library folders answers the health check, and every folder that answers SHALL record when it was last seen. On Linux a library folder below `/media/<user>/<name>`, `/run/media/<user>/<name>` or `/mnt/<name>` SHALL NOT answer while that folder exists but is not a mount point (the drive is unmounted).

#### Scenario: Two folders on one drive
- **WHEN** `E:\Movies` and `E:\TV` are library folders
- **THEN** one drive E: is listed with both folders

#### Scenario: Two folders on one Linux disk
- **WHEN** `/media/alex/Films/Movies` and `/media/alex/Films/TV` are library folders on Linux
- **THEN** one drive "Films" is listed with both folders

#### Scenario: Unmounted Linux disk
- **WHEN** the disk mounted at `/mnt/films` is unmounted, leaving the empty folder `/mnt/films`, and `/mnt/films` is a library folder
- **THEN** the drive is offline, its titles keep their state, and a scan does not mark them missing

### Requirement: Verified copy across volumes
A move across volumes SHALL copy one file at a time as `<name>.cvpart` into a hidden staging folder next to the destination (`.cvmove-<batch>-<id>`, which scans skip), hashing (SHA-256) 8 MiB chunks on a reader thread while the writer writes the one before. Each copy SHALL be flushed, read back and matched by size and hash. Then the staging folder SHALL become the destination folder in one rename, and the library SHALL be re-pointed in the same transaction.

#### Scenario: The disk kept something else
- **WHEN** the copy of `Example Movie.mkv` reads back with one byte different from the original
- **THEN** the copy is removed, the title is taken back to how it was, and the batch stops with "The copy of Example Movie.mkv does not match the original"

#### Scenario: Uncached read-back
- **WHEN** a copy is read back on Windows or Linux
- **THEN** the read bypasses the file cache with aligned whole blocks (`FILE_FLAG_NO_BUFFERING` on Windows, `O_DIRECT` on Linux), falling back to a normal read where that is refused; on macOS it is a normal read

#### Scenario: Last block of an odd size
- **WHEN** a file of 3 × 8 MiB + 5 bytes is read back without the cache
- **THEN** all its bytes are read, the last 5 included, and the hash matches

## ADDED Requirements

### Requirement: Recovery waits for an unmounted Linux drive
On Linux, recovering an interrupted rename or move batch SHALL treat a drive as reachable only when its mount root (`/media/<user>/<name>`, `/run/media/<user>/<name>`, `/mnt/<name>`, or `/` for other paths) is a mount point. A batch on an unmounted drive SHALL wait until the drive is back, as for a disconnected drive on Windows.

#### Scenario: Batch on an unmounted disk
- **WHEN** CineVault starts after a rename batch on `/media/alex/Films` was interrupted and that disk is not mounted
- **THEN** the batch is left waiting, nothing is written below `/media/alex/Films`, and it is settled once the disk is mounted again
