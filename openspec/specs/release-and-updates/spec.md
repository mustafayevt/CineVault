# release-and-updates Specification

## Purpose
Covers how CineVault is built, released and updated: the installers per platform, the version kept equal across the project files, the release workflows run by hand, the signed update manifest published to a public releases repository, the private signing key kept out of the repository, and the in-app update check, download, verification and install on Windows and macOS, including the Updates section in Settings and installing on close.

## Requirements

### Requirement: Installers per platform
A Windows release SHALL consist of an MSI package, an NSIS setup program (`CineVault_<version>_x64-setup.exe`, installed per user) and a portable zip (`CineVault_<version>_x64-portable.zip`, holding `CineVault.exe` and an empty `portable` marker file). A macOS release SHALL be one universal DMG for Apple Silicon and Intel; macOS SHALL have no portable package. A Linux release (x86_64, marked beta) SHALL consist of an AppImage (`CineVault_<version>_amd64.AppImage`), a `.deb` package (`CineVault_<version>_amd64.deb`) and an `.rpm` package (`CineVault-<version>-1.x86_64.rpm`); Linux SHALL have no portable package. `bun run tauri build` SHALL make the platform's installers and `bun run package:portable` the portable zip.

#### Scenario: Windows release files
- **WHEN** the Windows release workflow finishes for version 1.2.0
- **THEN** the release holds the MSI, `CineVault_1.2.0_x64-setup.exe`, `CineVault_1.2.0_x64-portable.zip` and `SHA256SUMS-Windows.txt`

#### Scenario: Linux release files
- **WHEN** the Linux release workflow finishes for version 1.2.0
- **THEN** the release holds `CineVault_1.2.0_amd64.AppImage`, `CineVault_1.2.0_amd64.deb`, `CineVault-1.2.0-1.x86_64.rpm` and `SHA256SUMS-Linux.txt`

### Requirement: Builds carry no secrets
The build SHALL contain no API keys, tokens or signing material. `bun run bundle:check` SHALL fail when the bundle configuration adds bundled resources, external binaries, the Tauri updater plugin or code-signing settings, or when a workflow reads a repository secret other than the built-in GitHub token.

#### Scenario: Secret in a workflow
- **WHEN** a workflow file reads `secrets.SOME_TOKEN`
- **THEN** `bun run bundle:check` fails and names the file

### Requirement: Versions kept equal
The version SHALL be the same in `src-tauri/tauri.conf.json`, the root `package.json`, `ui/package.json` and the root `Cargo.toml` (`[workspace.package] version`), and a version bump SHALL change all four together. `bun run bundle:check` SHALL fail when the Cargo workspace version and the Tauri configuration version differ. The release workflows SHALL read the version from `src-tauri/tauri.conf.json`.

#### Scenario: Mismatched version
- **WHEN** `Cargo.toml` says 1.2.0 and `src-tauri/tauri.conf.json` says 1.1.0
- **THEN** `bun run bundle:check` fails with a version mismatch and the release build stops

### Requirement: Release workflows run by hand
`.github/workflows/release-windows.yml`, `.github/workflows/release-macos.yml` and `.github/workflows/release-linux.yml` SHALL start only by hand (`workflow_dispatch`), each on its own; pushes and tags SHALL never start them. All three SHALL use `.github/workflows/release-build.yml`, which checks the bundling configuration, builds the installers, checks that they exist, makes the portable zip on Windows and writes `SHA256SUMS-<os>.txt`. The Linux build SHALL run on Ubuntu 22.04. They SHALL use only the built-in GitHub token.

#### Scenario: Push to main
- **WHEN** a commit is pushed to `main`
- **THEN** no release workflow starts

### Requirement: Release files on the GitHub Release
With the `publish` input on (the default), the release build SHALL put the files on the GitHub Release `v<version>`, creating it when missing and replacing that system's files from an earlier run of the same version. With `publish` off it SHALL keep the files as workflow artifacts for one day.

#### Scenario: Rebuild one system
- **WHEN** the macOS workflow is run again for a version that already has Windows and macOS files
- **THEN** only the macOS files on Release `v<version>` are replaced and the Windows files stay

#### Scenario: Build without publishing
- **WHEN** the Windows workflow is run with `-f publish=false`
- **THEN** the installers are kept as workflow artifacts and no release is created or changed

### Requirement: Checked release files
`bun run release:publish` SHALL download every file of Release `v<version>` (version from `src-tauri/tauri.conf.json` or `--version`) and check every installer against the `SHA256SUMS-<os>.txt` files. It SHALL publish nothing when a file does not match or a listed file is missing.

#### Scenario: Checksum mismatch
- **WHEN** a downloaded installer does not match its line in `SHA256SUMS-Windows.txt`
- **THEN** nothing is signed or uploaded

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

### Requirement: Private signing key never in the repository
The update-signing private key SHALL live only on the maintainer's machine, read from `--key`, else `CINEVAULT_UPDATE_KEY`, else `~/.cinevault-keys/cinevault-update.pem`. It SHALL never be in the repository, a CI secret or a workflow, and the tools SHALL only read it, never print or copy it. Only the public key SHALL be built into the app (`BUILT_IN_PUBLIC_KEY`), and tests SHALL generate their own key pairs.

#### Scenario: Key never printed
- **WHEN** `bun run release:publish` signs a manifest
- **THEN** its output shows no part of the private key

### Requirement: Key pair tool
`bun run release:keys -- keygen --out <file>` SHALL create an ed25519 key pair, write the private key only to a path outside the repository that does not exist yet, and print the public key. `bun run release:keys -- sign` SHALL wrap a hand-written payload in the signed envelope, for a test channel.

#### Scenario: Key path inside the repository
- **WHEN** `bun run release:keys -- keygen --out ./update.pem` is run inside the repository
- **THEN** it refuses and writes no key

### Requirement: Update check schedule
The app SHALL look for updates when `general.check-updates` ("Check for updates") is on, which is the default, at the interval `general.update-interval` (`daily` by default, `weekly` or `monthly`). It SHALL ask 15 seconds after start and then every hour, and contact the channel only when the interval has passed since the last successful read (`updates.last-check`). A failed check SHALL not count, so it is tried again later.

#### Scenario: Checks off
- **WHEN** "Check for updates" is off
- **THEN** the app sends no request to the channel by itself

#### Scenario: Failed check retried
- **WHEN** a check fails because the network is down
- **THEN** the last-check time is not updated and the next hourly ask contacts the channel again

### Requirement: Update channel
The channel SHALL be the address in `general.update-channel` ("Update channel") when it holds one, and the built-in channel, the manifest of the latest release in the project repository (`mustafayevt/CineVault`), when it is empty. The manifest SHALL be verified with `general.update-public-key` when set, else the built-in key; with no key at all no request SHALL be made.

#### Scenario: Fresh install uses the built-in channel
- **WHEN** the update channel setting is empty and the interval has passed
- **THEN** the app reads the signed manifest from the built-in channel and verifies it with the built-in key

### Requirement: Update channel fallback
The earlier releases repository (`mustafayevt/CineVault-releases`) SHALL be the fallback for the built-in channel only, when it cannot be reached or answers an error.

#### Scenario: Project repository unreachable
- **WHEN** the built-in channel answers 404 or the network fails
- **THEN** the app reads the earlier releases repository's manifest and verifies it with the same key

#### Scenario: Bad signature does not fall back
- **WHEN** the built-in channel's manifest is signed by another key
- **THEN** the check fails and the other repository is not asked

#### Scenario: Own channel does not fall back
- **WHEN** a channel set in Settings cannot be reached
- **THEN** the check fails and no other channel is asked

### Requirement: Update notice
A background check that finds a newer version SHALL show a toast "CineVault <version> is available" with "Show", which opens Settings › Updates, where the release notes of that version are shown as plain text above "Update now". Nothing SHALL be downloaded or installed until the user presses "Update now", unless installing on close is on. A failed background check SHALL interrupt no one.

#### Scenario: Update found in the background
- **WHEN** a newer signed release is published and checks are on
- **THEN** a toast says "CineVault 1.2.0 is available", and nothing is installed until the user presses "Update now"

#### Scenario: Release notes shown
- **WHEN** the offered release carries notes
- **THEN** Settings › Updates shows them under the status line before the user decides

### Requirement: Secure manifest transport
The channel address and every redirect SHALL be https; plain http SHALL be allowed only to `127.0.0.1`, `localhost` or `[::1]`, and never after an https hop. At most 10 redirects SHALL be followed, each checked like the first. The same rules SHALL apply to installer downloads.

#### Scenario: The channel redirects to the file
- **WHEN** the channel and the installer addresses answer with https redirects
- **THEN** the app follows them, verifies the manifest, and downloads and checks the installer as usual

#### Scenario: Redirect to plain http
- **WHEN** the channel or the installer address redirects to a plain http address
- **THEN** the app does not follow it ("The update server redirected to …, which is not https, so it was not followed."), nothing is offered or installed, and the check counts as failed

### Requirement: Manifest verification
The ed25519 signature SHALL be verified over the exact payload bytes before the payload is parsed. A wrong, missing or malformed signature SHALL be refused with "The update information is not signed by the expected key, so it was ignored. Nothing was installed." Only a version greater than the running one (semver, a prerelease being older than its release) that lists a build for this platform SHALL be offered.

#### Scenario: Tampered payload
- **WHEN** the payload was changed after signing
- **THEN** the manifest is refused and no update is offered

#### Scenario: Same version
- **WHEN** the manifest names the version that is running
- **THEN** no update is offered

### Requirement: Verified download
Installing SHALL name a version from a verified offer, never an address, so only a build listed in a verified manifest can be downloaded. The download SHALL go into `tmp/updates` in the data folder with progress, SHALL be cancellable, and SHALL match the signed size and SHA-256 or be deleted. Earlier downloads SHALL be swept when the app starts.

#### Scenario: Damaged download
- **WHEN** the downloaded installer's SHA-256 differs from the signed value
- **THEN** the file is deleted, nothing starts, and the error is shown

### Requirement: Only a verified setup starts
The app SHALL start only a file that matched the signed size and SHA-256, inside its own download folder, opened so no other program can write, rename or delete it, hashed again just before it starts and started while that handle is held. The arguments SHALL be fixed strings and the running executable's folder, never anything from the manifest. If the setup cannot start or the file changed, the app SHALL keep running and show the error.

#### Scenario: File changed after verification
- **WHEN** the downloaded setup is replaced after it was verified
- **THEN** the setup is not started, the error is shown in Settings › Updates and the app keeps running

### Requirement: Silent update on Windows
A Windows copy installed by the CineVault setup (an `uninstall.exe` beside the executable) whose download is the setup program SHALL install a confirmed update by itself: it SHALL start the verified setup with `/P /R /UPDATE /D=<folder of the running executable>`, show "Installing CineVault <version>…", and quit through its normal shutdown about 1.2 seconds later.

#### Scenario: Update now
- **WHEN** the user presses "Update now" on a copy installed by the setup and no job is running
- **THEN** the download progress shows, then "Installing CineVault 1.2.0…", the app quits, the setup installs without pages, and the new version starts with the library, settings and pinned icon unchanged

### Requirement: Passive setup keeps the install
In update mode the Windows setup SHALL wait up to 90 seconds for the app to exit, show no wizard pages, not run the old uninstaller, and restart the app when it is done. It SHALL keep the shortcuts, pins, data folder, settings and stored keys.

#### Scenario: App still closing
- **WHEN** the setup starts while the app is still shutting down
- **THEN** the setup waits until the app has exited before replacing its files

### Requirement: A running job is asked about
When a job is queued or running, "Update now" SHALL NOT download or start anything. Settings › Updates SHALL show "A job is running." with "Updating restarts CineVault, which stops the running work." and offer "Update after it finishes", "Update now and stop it" and "Not now". "Update after it finishes" SHALL check every 2 seconds and install once no job blocks the update.

#### Scenario: Wait for the job
- **WHEN** the user picks "Update after it finishes" while a scan runs
- **THEN** the status reads "CineVault 1.2.0 installs when the running jobs finish." and the install starts once the scan ends

### Requirement: Copies that open the installer
A portable copy, a copy not installed by the CineVault setup (an MSI install or a build run from a folder), a download that is not the setup program, and macOS SHALL NOT install over themselves: the app SHALL open the downloaded installer instead and say why. On macOS the sentence SHALL be "The disk image was opened. Drag CineVault to Applications to replace this version." A Linux copy that is not a writable AppImage (a `.deb` or `.rpm` install, an unpacked build, an AppImage in a folder the user cannot write) SHALL open the public release page instead and say "This copy was installed from a package, so it cannot replace itself. The release page was opened; download the new .deb or .rpm there."

#### Scenario: Portable copy
- **WHEN** the user presses "Update now" in a portable copy
- **THEN** the app opens the downloaded setup and says that it does not install over a portable copy and that the portable zip keeps the copy portable

#### Scenario: Debian package copy
- **WHEN** the user presses "Update now" in a copy installed from the `.deb`
- **THEN** the release page opens in the browser, the sentence above is shown, and the app keeps running

### Requirement: Windows setup keeps icons and pins
The Windows setup SHALL tell the shell that icons changed when it finishes, so pinned taskbar and Start shortcuts show the CineVault icon. Running the setup over an existing install, from the wizard or from an update, SHALL keep that install's Start menu and taskbar pins and shortcuts; uninstalling SHALL still remove them.

#### Scenario: Wizard reinstall
- **WHEN** the user runs a newer setup by hand and lets it remove the old version first
- **THEN** the pinned CineVault icon still starts the app after the install

### Requirement: Updates section in Settings
Settings › Updates SHALL show "CineVault <version>" with a status line: "Not checked yet", "Checking for updates…", "Up to date · checked today at <time>" (or "checked <date> at <time>"), "CineVault <version> is available · …", "Downloading CineVault <version> · <percent>%", or the last error. It SHALL offer "Check now", which ignores the toggle and the interval, "Update now" when an update is offered, and "Cancel" during a download.

#### Scenario: Version in Settings
- **WHEN** the user opens Settings › Updates on 1.0.1 after a check this morning that found nothing
- **THEN** it reads "CineVault 1.0.1" and "Up to date · checked today at 09:12"

#### Scenario: Check now finds an update
- **WHEN** the user presses "Check now" and 1.2.0 is published
- **THEN** the status reads "CineVault 1.2.0 is available" with the check time and "Update now" appears

### Requirement: Install updates when the app closes
Settings › Updates SHALL offer "Install updates when I close the app" (`updates.install-on-close`), off by default. While it is on, a found update SHALL be downloaded and verified in the background, and closing the main window of a copy installed by the setup SHALL start the same verified setup as "Update now" without asking. Other copies SHALL do nothing on close. A failed start SHALL be logged and the app SHALL close as usual.

#### Scenario: Update on close
- **WHEN** the option is on, 1.2.0 has been downloaded and verified, and the user closes the window
- **THEN** the update installs and the next start is 1.2.0

#### Scenario: Option off
- **WHEN** the option is off and an update is offered
- **THEN** closing the app installs nothing

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
