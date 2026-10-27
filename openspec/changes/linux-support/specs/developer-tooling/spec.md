## MODIFIED Requirements

### Requirement: CI workflow
`.github/workflows/ci.yml` SHALL run on pull requests to `main` and by hand (`workflow_dispatch`); pushes SHALL NOT start it. It SHALL run on Windows, macOS and Ubuntu 22.04: the UI install and build and the Rust format, clippy and test checks on all three (Ubuntu installs the WebKitGTK 4.1, GTK, D-Bus and OpenSSL development packages first), and the UI lint, typecheck, format and test checks on macOS. It SHALL need only read access to the repository contents.

#### Scenario: Started by hand
- **WHEN** `gh workflow run ci.yml` is run
- **THEN** the checks run on Windows, macOS and Ubuntu and a failing check fails the run

### Requirement: Pinned development tools
`bun run tools:fetch` SHALL download the pinned MediaInfo, FFmpeg and indexer service for the system it runs on (Windows, macOS or Linux x86_64) into the git-ignored `.dev-tools/` folder (or the folder in `CINEVAULT_TOOLS_DIR`), with the same manifest, size and SHA-256 checks as the app. It SHALL accept package names (`mediainfo`, `ffmpeg`, `indexer-service`), `--dir <folder>`, `--force` and `--help`. Media tests that need a tool SHALL use that folder and skip when the tool is missing. Downloaded binaries SHALL stay out of git.

#### Scenario: Fetch one tool
- **WHEN** `bun run tools:fetch -- ffmpeg` is run
- **THEN** only FFmpeg is downloaded into `.dev-tools/`, and a download whose checksum differs installs nothing

#### Scenario: Fetch on Linux
- **WHEN** `bun run tools:fetch` is run on Ubuntu x86_64
- **THEN** the `linux-x86_64` builds of MediaInfo, FFmpeg and the indexer service are in `.dev-tools/`
