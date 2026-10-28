## MODIFIED Requirements

### Requirement: Verified tool downloads
Downloads SHALL be pinned in a built-in manifest with an exact https URL, size and SHA-256 per platform (`windows-x86_64`, `macos-x86_64`, `macos-aarch64` and `linux-x86_64`). Archives SHALL be zip, tar.gz, tar.bz2 or tar.xz, or the program itself. A download SHALL be hashed while written to a staging folder, discarded on a mismatch, and only then unpacked; the finished folder SHALL replace the old install in one rename. A program-tree package SHALL be refused when an entry is absolute, holds `..`, a drive letter, a link or a special file, or lies outside its root.

#### Scenario: Checksum mismatch
- **WHEN** a downloaded FFmpeg archive does not match its expected SHA-256
- **THEN** it is deleted, nothing is installed, and the error says the download did not match its expected checksum

#### Scenario: Archive entry outside the package
- **WHEN** a downloaded program-tree archive holds an entry with `..` or an absolute path
- **THEN** nothing is installed and the install fails with a plain error

#### Scenario: Linux downloads
- **WHEN** the first-run setup finishes on Linux with the recommended tools ticked
- **THEN** `libmediainfo.so.0` and `ffmpeg` are downloaded from the `linux-x86_64` entries into the app's tools folder, and the Technical card reads video details
