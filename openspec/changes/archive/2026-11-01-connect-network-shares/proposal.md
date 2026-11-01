## Why

Many people keep their movies and TV shows on a NAS whose shares need a user name and a password that differ from the computer's own sign-in (issue #25). Today CineVault can only add a folder the operating system can already open, so they first have to connect the share outside the app, and when the connection is gone after a restart, their titles read offline with no hint why and no way to fix it from CineVault.

## What Changes

1. **"Network share…" in the desktop app.** Settings › Library folders and the onboarding Folders step gain "Network share…" next to "Add folder…". It opens a page with one Address field (`\\nas\movies\Films` or `smb://nas/movies/Films`, a folder inside the share optional), User, Password and "Remember the password" (on by default). CineVault connects the share with that login, checks the folder, and adds it as a library folder through the usual folder rules and type detection.
2. **One way per operating system.** Windows connects `\\server\share` with the given login, without a drive letter, and the folder keeps its UNC path (`\\nas\movies\Films`). macOS mounts the share with the system's SMB support under a folder CineVault owns. A Linux desktop uses the desktop's user-level share mounting where it is present; where it is not, the page says so and points to the system's own way of mounting a share.
3. **Passwords only in the credential store.** A remembered password lives only in the operating system credential store, never in the settings file, the database, a command line or the logs. A password that is not remembered is kept in memory until CineVault quits.
4. **Reconnect at start.** Remembered shares are connected again when the app starts, before the folder health check and the start-up scan, so their titles are online. A share that cannot be connected reads offline (never Missing) with the reason, such as a wrong password or an unreachable server, and a "Connect…" action to enter the password again.
5. **Forget a share.** When the last library folder on a share is removed and its Undo has passed, CineVault disconnects the share and deletes its stored password.
6. **Browser mode.** The server and the container run unprivileged and cannot connect shares. `scripts/install/install.sh` (the Linux service and `--docker`) and `scripts/install/install.ps1` (Docker Desktop) accept a share address as a library folder, ask for its user and password, and set up the connection on the host: a systemd mount with a root-only credentials file for the Linux service, and a CIFS volume or a host mount for containers. The README server guide and `docker-compose.yml` document the same. In a browser, "Network share…" explains that shares are connected on the server by the installer and shows the command to run.
7. **Drives.** A folder on a connected share belongs to that share's drive (`\\nas\movies`) wherever the operating system put it.

## Capabilities

### New Capabilities

- `network-shares`: connecting SMB shares with a user and password for library folders, keeping the password in the credential store, reconnecting at start, offline reasons and "Connect…", forgetting unused shares, and the server installer's share setup.

### Modified Capabilities

- `settings`: the Library folders section offers "Network share…".
- `onboarding`: the Folders step offers "Network share…".
- `drives`: a folder on a connected share belongs to that share, whatever its local mount path.

## Out of scope

- NFS, WebDAV, FTP, cloud storage and any protocol other than SMB.
- Browsing the network for servers or listing a server's shares; the user types the server and share names.
- Kerberos, domain single sign-on, smart cards and guest logins without a user name.
- Drive letters on Windows and mounts outside CineVault's own mount folder on macOS.
- Connecting shares from inside the server or the container (they stay unprivileged), and a browser form that sends a NAS password to the server.
- Connecting shares from the `cinevault` command line.
- Mounting a share on Linux desktops without user-level share mounting; the page points to the system's tools instead.

## Impact

- New crate `crates/cv-shares`: the connectors per operating system, driving the system's own tools with the password on standard input, and a fake for tests.
- `crates/cv-db`: a migration adding the `network_share` table (server, share, user, local root, remember flag); `data_source` is unchanged.
- `crates/cv-app`: a `NetworkShareService` (connect and add, reconnect at start, offline reasons, forget), the start-up order, the folder health reasons, drive grouping of share roots, and redaction of passwords that are not remembered.
- `crates/cv-ipc`, `src-tauri`: share commands and regenerated `ui/src/bindings.ts`; `crates/cv-web` answers them with `not_applicable`.
- `ui`: the Network share page, the buttons in Settings › Library folders and the Folders step, offline reasons with "Connect…", and the browser explanation.
- `scripts/install/install.sh`, `scripts/install/install.ps1`, `docker-compose.yml`, the README server guide.
- `CHANGELOG.md`, `site/changelog.html`, the screenshots and the landing page wording where Library folders appear.
