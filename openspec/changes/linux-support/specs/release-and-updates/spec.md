## MODIFIED Requirements

### Requirement: Installers per platform
A Windows release SHALL consist of an MSI package, an NSIS setup program (`CineVault_<version>_x64-setup.exe`, installed per user) and a portable zip (`CineVault_<version>_x64-portable.zip`, holding `CineVault.exe` and an empty `portable` marker file). A macOS release SHALL be one universal DMG for Apple Silicon and Intel; macOS SHALL have no portable package. A Linux release (x86_64, marked beta) SHALL consist of an AppImage (`CineVault_<version>_amd64.AppImage`), a `.deb` package (`CineVault_<version>_amd64.deb`) and an `.rpm` package (`CineVault-<version>-1.x86_64.rpm`); Linux SHALL have no portable package. `bun run tauri build` SHALL make the platform's installers and `bun run package:portable` the portable zip.

#### Scenario: Windows release files
- **WHEN** the Windows release workflow finishes for version 1.2.0
- **THEN** the release holds the MSI, `CineVault_1.2.0_x64-setup.exe`, `CineVault_1.2.0_x64-portable.zip` and `SHA256SUMS-Windows.txt`

#### Scenario: Linux release files
- **WHEN** the Linux release workflow finishes for version 1.2.0
- **THEN** the release holds `CineVault_1.2.0_amd64.AppImage`, `CineVault_1.2.0_amd64.deb`, `CineVault-1.2.0-1.x86_64.rpm` and `SHA256SUMS-Linux.txt`

### Requirement: Release workflows run by hand
`.github/workflows/release-windows.yml`, `.github/workflows/release-macos.yml` and `.github/workflows/release-linux.yml` SHALL start only by hand (`workflow_dispatch`), each on its own; pushes and tags SHALL never start them. All three SHALL use `.github/workflows/release-build.yml`, which checks the bundling configuration, builds the installers, checks that they exist, makes the portable zip on Windows and writes `SHA256SUMS-<os>.txt`. The Linux build SHALL run on Ubuntu 22.04. They SHALL use only the built-in GitHub token.

#### Scenario: Push to main
- **WHEN** a commit is pushed to `main`
- **THEN** no release workflow starts

### Requirement: Signed update manifest
The manifest payload SHALL hold `version`, `pub_date`, `notes` and `platforms`, with the NSIS setup program for `windows-x86_64`, a universal DMG for both `darwin-aarch64` and `darwin-x86_64`, and the AppImage for `linux-x86_64`, each with its URL, SHA-256 and size. `bun run release:publish` SHALL sign the exact payload bytes with ed25519 into an envelope `{ "payload", "signature" }` and verify it with the public key built into the app before uploading anything.

#### Scenario: Wrong signing key
- **WHEN** the private key given does not belong to the public key built into the app
- **THEN** the script refuses to publish

#### Scenario: Linux added later
- **WHEN** the script is run after the Linux workflow added the AppImage, `.deb` and `.rpm`
- **THEN** the manifest gains the `linux-x86_64` entry for the AppImage, the packages are uploaded but not in the manifest, and the other entries stay

### Requirement: Published to a public releases repository
`bun run release:publish` SHALL create the release in a public releases repository as a draft, upload the installers, the checksum files, version-less copies (`CineVault-setup-x64.exe`, `CineVault-x64.msi`, `CineVault-portable-x64.zip`, `CineVault-universal.dmg`, `CineVault-x86_64.AppImage`, `CineVault-amd64.deb`, `CineVault-x86_64.rpm`) and `manifest.json`, then publish it as the latest release. It SHALL publish the same files, notes and signed manifest to each repository given with `--repo <owner/name>` (repeatable; by default the project repository `mustafayevt/CineVault` and the earlier releases repository `mustafayevt/CineVault-releases`), with the installer addresses of the manifest pointing at the first one, and in the project repository it SHALL tag the release `v<version>` at the commit that is checked out. Running it again SHALL be safe. `--dry-run` SHALL do everything except the upload, and `--self-test` SHALL check the payload, notes and signing logic offline.

#### Scenario: macOS added later
- **WHEN** the script is run again after the macOS workflow added the DMG
- **THEN** the manifest gains the `darwin-aarch64` and `darwin-x86_64` entries and the Windows entry stays

#### Scenario: Stable download link
- **WHEN** a release is published
- **THEN** `releases/latest/download/CineVault-setup-x64.exe` in the public releases repository serves the newest setup

#### Scenario: Two repositories
- **WHEN** a release is published without `--repo`
- **THEN** both repositories get the same files and the same signed manifest, and the project repository's release is tagged at the checked-out commit

#### Scenario: Stable Linux link
- **WHEN** a release with Linux files is published
- **THEN** `releases/latest/download/CineVault-x86_64.AppImage` serves the newest AppImage

### Requirement: Copies that open the installer
A portable copy, a copy not installed by the CineVault setup (an MSI install or a build run from a folder), a download that is not the setup program, and macOS SHALL NOT install over themselves: the app SHALL open the downloaded installer instead and say why. On macOS the sentence SHALL be "The disk image was opened. Drag CineVault to Applications to replace this version." A Linux copy that is not a writable AppImage (a `.deb` or `.rpm` install, an unpacked build, an AppImage in a folder the user cannot write) SHALL open the public release page instead and say "This copy was installed from a package, so it cannot replace itself. The release page was opened; download the new .deb or .rpm there."

#### Scenario: Portable copy
- **WHEN** the user presses "Update now" in a portable copy
- **THEN** the app opens the downloaded setup and says that it does not install over a portable copy and that the portable zip keeps the copy portable

#### Scenario: Debian package copy
- **WHEN** the user presses "Update now" in a copy installed from the `.deb`
- **THEN** the release page opens in the browser, the sentence above is shown, and the app keeps running

## ADDED Requirements

### Requirement: AppImage replaces itself
A Linux copy run as an AppImage (the `APPIMAGE` environment variable names an existing file in a folder the user can write) SHALL install a confirmed update by itself: it SHALL copy the verified AppImage into the same folder as a hidden `.part` file, make it executable, check its SHA-256 against the signed manifest again, rename it over the running AppImage in one step, start the new AppImage and quit through its normal shutdown. "Install updates when I close the app" SHALL do the same on close.

#### Scenario: Update now in an AppImage
- **WHEN** the user presses "Update now" in `~/Applications/CineVault.AppImage` and no job is running
- **THEN** the download progress shows, then "Installing CineVault 1.2.0…", the app quits, and `~/Applications/CineVault.AppImage` starts as 1.2.0 with the library, settings and keys unchanged

### Requirement: Failed AppImage update keeps the old copy
When any step of an AppImage update before the final rename fails, the old AppImage SHALL stay unchanged, the `.part` file SHALL be removed, and the error SHALL be shown in Settings › Updates. A `.part` file left by an interrupted update SHALL be removed at the next start.

#### Scenario: Copy fails its check
- **WHEN** the copied AppImage does not match the signed SHA-256
- **THEN** the old AppImage is unchanged, no `.part` file is left, and Settings › Updates shows the error
