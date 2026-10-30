# Spec Delta

## MODIFIED Requirements

### Requirement: Update check schedule
In the desktop app, the app SHALL look for updates when `general.check-updates` ("Check for updates") is on, which is the default, at the interval `general.update-interval` (`daily` by default, `weekly` or `monthly`). It SHALL ask 15 seconds after start and then every hour, and contact the channel only when the interval has passed since the last successful read (`updates.last-check`). A failed check SHALL not count, so it is tried again later. The server SHALL NOT run this schedule; it follows "Server updates".

#### Scenario: Checks off
- **WHEN** "Check for updates" is off
- **THEN** the app sends no request to the channel by itself

#### Scenario: Failed check retried
- **WHEN** a check fails because the network is down
- **THEN** the last-check time is not updated and the next hourly ask contacts the channel again

#### Scenario: Server
- **WHEN** the server runs for a month
- **THEN** it never contacts the update channel by itself

### Requirement: Update notice
In the desktop app, a background check that finds a newer version SHALL show a toast "CineVault <version> is available" with "Show", which opens Settings › Updates, where the release notes of that version are shown as plain text above "Update now". Nothing SHALL be downloaded or installed until the user presses "Update now", unless installing on close is on. A failed background check SHALL interrupt no one. In a browser, no update toast or "Update now" SHALL be shown; Settings › Updates follows "Server updates".

#### Scenario: Update found in the background
- **WHEN** a newer signed release is published and checks are on in the desktop app
- **THEN** a toast says "CineVault 1.2.0 is available", and nothing is installed until the user presses "Update now"

#### Scenario: Release notes shown
- **WHEN** the offered release carries notes
- **THEN** Settings › Updates shows them under the status line before the user decides

#### Scenario: Browser
- **WHEN** an update is published while a browser is open
- **THEN** no update toast or install button appears in the browser

### Requirement: Updates section in Settings
In the desktop app, Settings › Updates SHALL show "CineVault <version>" with a status line: "Not checked yet", "Checking for updates…", "Up to date · checked today at <time>" (or "checked <date> at <time>"), "CineVault <version> is available · …", "Downloading CineVault <version> · <percent>%", or the last error. It SHALL offer "Check now", which ignores the toggle and the interval, "Update now" when an update is offered, and "Cancel" during a download. In a browser, the section SHALL show the running version, the image or package that updates it, the instruction to pull or install it, and a link to the release notes, with no "Update now" and no download controls.

#### Scenario: Version in Settings
- **WHEN** the user opens Settings › Updates on 1.0.1 in the desktop app after a check this morning that found nothing
- **THEN** it reads "CineVault 1.0.1" and "Up to date · checked today at 09:12"

#### Scenario: Check now finds an update
- **WHEN** the user presses "Check now" in the desktop app and 1.2.0 is published
- **THEN** the status reads "CineVault 1.2.0 is available" with the check time and "Update now" appears

#### Scenario: Browser settings
- **WHEN** Settings › Updates is opened in a browser
- **THEN** it shows the running version and the image or package instruction, with no Install button

### Requirement: Install updates when the app closes
In the desktop app, Settings › Updates SHALL offer "Install updates when I close the app" (`updates.install-on-close`), off by default. While it is on, a found update SHALL be downloaded and verified in the background, and closing the main window of a copy installed by the setup SHALL start the same verified setup as "Update now" without asking. Other copies SHALL do nothing on close. A failed start SHALL be logged and the app SHALL close as usual. In a browser, the setting SHALL NOT be shown and closing a tab SHALL install nothing.

#### Scenario: Update on close
- **WHEN** the option is on, 1.2.0 has been downloaded and verified, and the user closes the window
- **THEN** the update installs and the next start is 1.2.0

#### Scenario: Option off
- **WHEN** the option is off and an update is offered
- **THEN** closing the app installs nothing

#### Scenario: Browser
- **WHEN** a browser tab is closed
- **THEN** the server installs nothing

## ADDED Requirements

### Requirement: Server packages and container image
A Linux release SHALL add a server container image for amd64 and arm64 under `ghcr.io/<owner>/cinevault` with a semver tag and `latest`, a `cinevault-server` `.deb` and `.rpm` (distinct from the desktop `cinevault` package), and a tarball, all carrying the release's version. The release workflow SHALL build them from the same source as the desktop app and publish the image to the registry and the packages with the release. The image SHALL run as a non-root user, SHALL accept `PUID`/`PGID`, SHALL mount the data folder and the library folders read-write, SHALL declare a health check and SHALL default the deleted-files destination to `.deletedByCineVault`. The packages SHALL contain the server, the `cinevault` command line and the interface assets. The repository SHALL hold a Docker Compose example and a systemd unit example. The version check SHALL cover the image tag and the package versions as well as the four project files.

#### Scenario: A release
- **WHEN** the release workflow finishes for version 1.2.0
- **THEN** the registry holds the amd64 and arm64 images tagged 1.2.0, and the release holds the server `.deb`, `.rpm` and tarball plus the desktop installers, all at 1.2.0

#### Scenario: Compose
- **WHEN** the Compose example is started with the data folder and `/media/movies` mounted read-write
- **THEN** the container is healthy, serves the interface and can rename a title in `/media/movies`

#### Scenario: Package names
- **WHEN** the server `.deb` and the desktop `.deb` are installed on the same machine
- **THEN** both install, because the server package is named `cinevault-server`

#### Scenario: Version disagreement
- **WHEN** the image tag, a package version and the project version differ
- **THEN** the release check fails

### Requirement: Server updates and rollback
The server SHALL be updated by pulling a new image or installing a new package. The release notes and the home-server documentation SHALL say that an upgrade may migrate the library, that rolling back needs a same-or-newer version, and that `library.db` and `settings.json` should be copied before an upgrade. A failed upgrade SHALL leave the old image or package runnable and the data untouched until the new version starts.

#### Scenario: Upgrade
- **WHEN** a running container is replaced with a newer image
- **THEN** the library, settings, owner file and provider keys survive and Settings shows the new version

#### Scenario: Rollback refused
- **WHEN** an older server is started on a library a newer server migrated
- **THEN** it refuses the library and says a same-or-newer version is needed

#### Scenario: Failed upgrade
- **WHEN** the new image exits while migrating
- **THEN** the old image still starts on the untouched data after the operator restores the pre-upgrade copy
