# Design

## Context

- A library folder is added by `add_data_source(kind, path)` (`src-tauri/src/sources_ipc.rs` → `crates/cv-ipc/src/sources.rs` → `DataSourceService::add` in `crates/cv-app/src/data_sources.rs`), which runs the folder rules (`SourceGuard::check_with_links`) and a reachability test (`Path::is_dir`, replaceable through `with_reachability`). The `data_source` table (`crates/cv-db/src/schema.sql`) holds `id, kind, path, position, created_ms, label, last_seen_ms` and no credentials.
- Folder health probes each folder with a 3-second timeout (`PROBE_TIMEOUT` in `crates/cv-app/src/scanning.rs`); a missing, unreadable or empty folder is offline and never compared (`library-scanning`).
- UNC paths already parse and group as network drives (`crates/cv-scan/src/sources.rs`, `cv-fs` `paths.rs`, `cv-app` `drives.rs::volume_key`). Linux mount roots below `/media`, `/run/media` and `/mnt/<name>` are drives, and such a folder that is not a mount point reads offline. A macOS path outside `/Volumes` and the Linux user-mount path `/run/user/<uid>/gvfs/…` fall through to the root drive today.
- The folder guard refuses the home folder itself, `~/Library` and everything below it, and CineVault's data folder and everything below it, so a mount folder cannot live in either.
- Secrets go through `SecretStore` (`crates/cv-app/src/secrets.rs`) to the OS credential store under `CineVault` (or `CineVault portable <id>`); every value that passes through it is registered with `redact::register_secret`. On Linux without a Secret Service, `set` fails with `SecretError::NoKeyring`.
- `unsafe` is denied workspace-wide, with one scoped exception for the MediaInfo FFI module. The Windows connection API (`WNetAddConnection2W`) and macOS `NetFS` are FFI.
- Browser mode: `packaging/cinevault-server.service` runs as an unprivileged user with `NoNewPrivileges=true`; `packaging/entrypoint.sh` drops root before the server starts. Neither can mount. The installer (`scripts/install/install.sh`, `install.ps1`) already asks for library folders (`ask_media` / `Ask-Media`), writes a systemd drop-in with `CINEVAULT_WEB_ROOTS`, and binds host folders into the container at `/media/<name>`.

## Goals / Non-Goals

**Goals:** a share with its own login added in one page; no password outside the credential store and process memory, and never on a command line; titles of a remembered share online after a restart; an offline share that says why and can be fixed in place; no new `unsafe`; nothing in a media folder written, moved or deleted by connecting or forgetting a share.

**Non-Goals:** a general network-filesystem layer, an SMB client inside CineVault (every file access stays a plain OS path), changes to how scans, renames or moves treat network paths.

## Decisions

### D1. Drive the operating system's own tools; no FFI

A new crate `crates/cv-shares` holds a `ShareConnector` trait (`connect`, `is_connected`, `disconnect`, `local_root`) with one implementation per OS and a fake for tests. Every implementation starts a system tool with a fixed argument list and writes the password to that tool's standard input (or its terminal, D4), then drops the buffer. Arguments carry only the server, share, user and paths, never the password; a unit test per connector asserts this on the exact `Command` it builds.

*Alternative:* `WNetAddConnection2W` through the `windows` crate. It is the cleanest Windows path (typed errors, no extra process, about 50 ms instead of the 0.5 to 1 s PowerShell needs to start), but it is `unsafe` FFI. It is listed under Open Questions as a second scoped exception the owner may prefer.

*Alternative:* a pure-Rust SMB client library. Rejected: every file operation (scan, NFO and artwork writes, rename, move, delete, the player) would need a second I/O path.

### D2. Windows: `New-SmbMapping` without a drive letter

`powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command <fixed script>` with the script text constant in the binary. The script sets `[Console]::InputEncoding` to UTF-8, reads the password with `[Console]::In.ReadLine()`, calls `New-SmbMapping -RemotePath \\server\share -UserName <user> -Password $p -Persistent $false` (no `-LocalPath`, so no drive letter) and prints one JSON line `{ok, code, message}` from the CIM error's Win32 code. Server, share and user are passed as `-Command` arguments after validation (D7), never interpolated into script text.

Error mapping: 1326, 86 and 2202 → wrong user or password; 53 and 1231 → server not reachable; 67 → share not found; 1219 → already connected to this server as another user ("Windows is already connected to nas as another user. Disconnect it in File Explorer, or use the same user."); 1909 → account locked; anything else → the system message with its code.

Before connecting, `is_connected` lists the user's SMB mappings (`Get-SmbMapping -RemotePath`) and reads the folder with the 3-second probe; an already working path is not connected again. Disconnect is `Remove-SmbMapping -RemotePath … -Force:$false`; open files make it fail, which is accepted (D9). A non-persistent mapping lasts until the user signs out, which is what "Shares stay connected" needs; nothing is written to the registry and Windows stores no credential.

Trade-off: a PowerShell start per connect and per reconnect at start (run in parallel per server, off the UI thread), and the SmbShare module must exist (Windows 8 and later; CineVault supports Windows 10 and 11). A machine whose policy blocks PowerShell gets the system message.

### D3. Library paths and the share table

A new table `network_share (id, server, share, user, local_root UNIQUE, remember, created_ms, last_connected_ms, last_error, unused_since_ms)` with a migration (the existing backup-before-migration applies). `data_source` is unchanged: a library folder belongs to a share when its path equals or lies below the share's `local_root`, compared with the path rules of the OS (case-insensitive on Windows). This keeps "Add folder…" on an already connected `\\nas\movies\TV` working without the Network share page, and needs no join to show a row.

`local_root` per OS: Windows `\\nas\movies`; macOS `~/CineVault Shares/nas/movies` (D4); Linux the user-mount path reported by `gio info` (D5), such as `/run/user/1000/gvfs/smb-share:server=nas,share=movies`. The user name (with an optional `DOMAIN\` part) is stored, because it is shown and is not a secret.

`drives.rs` asks the share table first: a path below a share's `local_root` has the volume key `\\server\share` and kind network share, so the drive is the same on every OS (the `drives` delta).

### D4. macOS: `mount_smbfs` on a pseudo-terminal under `~/CineVault Shares`

The mount folder is `~/CineVault Shares/<server>/<share>` (made with mode `0700`), outside `~/Library` and the data folder, which the folder guard refuses. CineVault starts `/sbin/mount_smbfs //<user>@<server>/<share> <folder>` on a pseudo-terminal it owns (a maintained crate such as `portable-pty`, whose `unsafe` stays inside the dependency) and writes the password when the `Password:` prompt appears. The pseudo-terminal is used because `mount_smbfs` reads the password from the controlling terminal, not from a pipe, and a URL with `user:password@` would put it in the process list. Exit status and the last stderr line map to the same errors as D2 (`Authentication error`, `server connection failed`, `No such file or directory` for the share). Disconnect is `/sbin/umount <folder>`.

*Alternative:* `osascript` reading `mount volume "smb://…" as user name … with password …` from standard input. It mounts in `/Volumes/<share>` like Finder, with a name that may get a `-1` suffix, and can show a system error dialog CineVault does not control. Kept as the fallback if the pseudo-terminal proves fragile on a new macOS version.

### D5. Linux desktop: `gio mount`

Detection: `gio` on `PATH`, a session bus, and `gio mount -l` succeeding; otherwise the page shows the "no user-level share mounting" explanation (the `network-shares` requirement) with the command for the system's own mount (`sudo mount -t cifs` with a credentials file, or the distribution's file manager). Connect: `gio mount smb://<server>/<share>` with the prompts answered on standard input (user, domain, password; the prompt text is read before each answer, so a changed order fails instead of sending the password to the wrong prompt). The local path comes from `gio info smb://<server>/<share>/` (`local path:`). Disconnect: `gio mount -u smb://<server>/<share>`. `gio` saves no password itself (it never offers "remember forever" on the command line).

The user-mount path is a FUSE mount that the deleted-files code already treats as a network file system (`fuse.gvfsd-fuse`), so deletes on it follow the existing network rules.

### D6. Passwords

- **Store:** `SecretStore` account `network-share.<id>`. The share row is inserted first; if the credential cannot be written the row is rolled back, except when the user chose "Connect without remembering" (row with `remember = 0`).
- **Not remembered:** a `Secret` held in the `NetworkShareService` map for the life of the process, registered with `redact::register_secret` when entered and unregistered when forgotten. The value redaction applies to secrets of at least 6 characters; a shorter password is still never formatted into a log line or error (the `Secret` type's `Debug` and `Display` print `[REDACTED]`, and the IPC input type for the form implements `Debug` without the password).
- **Tool output:** stdout and stderr of the tools are captured, passed through `redact::redact`, and only the mapped error or the last stderr line is logged.
- **Portable copy:** the share passwords follow the copy's own credential-store service, like its provider keys.

### D7. Input checks

Server: a host name (letters, digits, `-`, `.`), an IPv4 address or a bracketed IPv6 address. Share: 1 to 80 characters without `\ / : * ? " < > |` and control characters. Folder: a relative path without `..` or empty parts. User: printable, no control characters, an optional `DOMAIN\` prefix. Anything else is refused on the page before a tool starts. These checks also keep tool arguments unambiguous.

### D8. Start-up order

`AppServices::start_background` runs a `reconnect_shares` step before the folder health check and the start-up scan: every remembered share is checked (`is_connected`) and connected if needed, in parallel per server, each limited to 15 seconds. Folders on other volumes do not wait: the health check and scan of folders not below a share's `local_root` start at once; folders of a share start after its step ends. A share that failed records `last_error` (a code, never a message holding user input) and its folders read offline with the reason. A later successful "Connect…" scans that share's folders.

Health: a folder below a share whose last connect failed reads offline with the share's reason. On macOS, a folder below `~/CineVault Shares/<server>/<share>` while that folder is not a mount point reads offline (the same rule Linux already has for `/mnt/<name>`), so an empty mount folder can never look like an emptied library.

### D9. Forgetting a share

Removing a folder sets `unused_since_ms` on its share when no other folder remains below it; Undo (re-adding the folder) clears it. A sweep runs one minute after the removal (longer than any Undo toast) and at every start: for each unused share, it waits until no job (scan, rename, move, artwork or NFO write) has a path below the share, then disconnects, deletes the credential and the row. When disconnecting fails because files are open (a player), the row and the credential are still deleted and the OS connection ends at sign-out.

### D10. Browser mode: connected on the host by the installer

The server never mounts. The share commands are desktop-only and answer `not_applicable` from the server's dispatcher; the browser's "Network share…" shows the explanation with the installer command for the server's OS (`curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh`, choosing "Change which movie and TV folders CineVault may use"; on Windows with Docker Desktop, `irm https://mustafayevt.github.io/CineVault/install.ps1 | iex`).

The installer accepts `//nas/movies[/Films]`, `\\nas\movies[\Films]` or `smb://nas/movies[/Films]` wherever it asks for a folder (and in `CINEVAULT_MEDIA`). For each share it asks "User for //nas/movies" and "Password" (read with `stty -echo` from `/dev/tty`, or `Read-Host -AsSecureString`). Unattended runs take `CINEVAULT_SHARE_USER` and `CINEVAULT_SHARE_PASSWORD_FILE` (a file path, never the password in an environment variable).

- **Linux service:** installs `cifs-utils` if missing (asking first), writes `/etc/cinevault/shares/<server>-<share>.cred` (`username=`, `password=`, optional `domain=`; owner root, mode `0600`, written through a temporary file in the same folder and renamed, with the content on standard input of `install -m 0600 /dev/stdin`), and a mount unit `mnt-<server>\x2d<share>.mount` (name from `systemd-escape --path`) for `/mnt/<server>-<share>`: `What=//server/share`, `Type=cifs`, `Options=credentials=…,uid=<run user>,gid=<group>,file_mode=0664,dir_mode=0775,iocharset=utf8,vers=3.0,_netdev,nofail`, `WantedBy=remote-fs.target`. `/mnt/<name>` is chosen because the drives code already treats it as a drive and reads it offline while it is not a mount point. The service drop-in gains `Wants=` and `After=` on the mount (not `Requires=`, so a NAS that is off never stops the service) and the mount path in `CINEVAULT_WEB_ROOTS`. The installer starts the mount, checks it is writable by the service user, and reports the mount's own error when it fails.
- **Linux with `--docker`:** the same host mount and credentials file, then a bind `/mnt/<server>-<share>:/media/<name>:rslave`, so a share mounted after the container started appears inside it. The password is never stored in Docker.
- **Docker Desktop (Windows `install.ps1`, macOS `install.sh`):** the host's SMB connection is not visible inside Docker's VM, so the installer creates a local-driver CIFS volume `cinevault-<server>-<share>` (`type=cifs`, `device=//server/share`, `o=username=…,password=…,uid=<PUID>,gid=<PGID>,file_mode=0664,dir_mode=0775,vers=3.0`) mounted at `/media/<name>`. To keep the password off the command line, the volume is created through the Docker Engine API with the JSON body sent over the local socket (`curl --unix-socket … --data-binary @-` on macOS) or the `docker_engine` named pipe (in-process in PowerShell). The password stays in Docker's volume options, readable to anyone who can run `docker volume inspect`, which on these machines is the signed-in owner; the scripts say so.
- **Changing folders and uninstalling** stop and remove the mount units, credentials files and volumes of shares no longer listed, and remove a mount folder only with `rmdir` after `mountpoint -q` says it is not mounted. Nothing ever runs a recursive delete under a mount folder.
- `docker-compose.yml` gains a commented `volumes:` example with `driver_opts` (`type: cifs`, `device`, `o: "username=${NAS_USER},password=${NAS_PASSWORD},uid=${PUID},gid=${PGID},vers=3.0"`) and says to keep the values in an `.env` file with mode `0600`. The README server guide gets a "Library folders on a NAS" part covering the installer, the compose example and the manual systemd mount.

### D11. Data safety

Connecting, reconnecting and forgetting a share write, move and delete nothing in a media folder. The files CineVault writes are the library database row (in the database's transaction), the credential (the OS store), and on macOS the empty mount folders. An interrupted connect leaves at worst a mounted share with no row: at start, a mount below `~/CineVault Shares` (or a gvfs mount CineVault started, recorded in the row before connecting) with no matching row is unmounted, and its empty folder removed with `rmdir`. Mount folders are never removed recursively, and are removed only after the OS reports them as not mounted, so an unmount that failed can never lead to deleting files on the NAS. The installer's credentials files and units are written to a temporary name and renamed. Adding a share folder is undone by removing the folder (Undo toast as today); forgetting a share is undone by connecting it again. Scans, renames, moves and deletes on share folders are unchanged and keep their own journals and Undo.

### D12. Network share page (mock-up note)

A page, like the other pickers (no dialog), opened from "Network share…" with Back and Esc returning to where it was opened.

- **Title:** "Connect a network share" (reconnect: "Connect to \\nas\movies").
- **Fields, in order:** Address (placeholder `\\nas\movies`; hint "You can paste \\nas\movies or smb://nas/movies, with a folder inside it if you like"), a one-line preview of the library folder path in mono (`\\nas\movies\Films`, or `~/CineVault Shares/nas/movies/Films`), User (`alex`; hint "DOMAIN\name also works"), Password with a Show/Hide icon button (`aria-label`), "Remember the password" checkbox, on, with the store named ("Kept in Windows Credential Manager" / "macOS Keychain" / "the system keyring"). One field instead of Server, Share and Folder: people paste the address they already see in Explorer or Finder (owner's choice, 2026-11-01).
- **Buttons:** "Connect and add" (primary; reconnect: "Connect"), "Cancel".
- **Busy:** the button reads "Connecting…", the fields are read-only, Esc cancels and stops the tool.
- **Errors, inline under the field concerned:** Password: "Wrong user name or password." Address: "nas could not be reached. Check the name or address and that it is switched on.", "There is no share named movies on nas." or "Films was not found on \\nas\movies." A banner above the buttons for: another user already connected (Windows), the keyring message with "Connect without remembering", the folder rules' refusal, or the system message.
- **Linux without user-level mounting:** the fields are replaced by the explanation, the mount command in mono with Copy, and "Add folder…".
- **Reconnect:** Address read-only, User editable, Password focused.
- **Library folder row** for an offline share folder: "offline · Wrong password · 120 titles kept" and a "Connect…" button next to Remove; an online one adds "on \\nas\movies as alex" in the secondary text.
- **Browser:** "Network share…" opens a popover anchored to the button: one paragraph that shares are connected on the server by the installer, the command in mono with Copy, and "Then add the folder with Add folder…".

## Risks / Trade-offs

- [PowerShell is slow to start or blocked by policy] → connects run off the UI thread with a busy state; reconnects at start run in parallel and only for shares not already connected; a blocked PowerShell shows the system message. The FFI alternative is in Open Questions.
- [`mount_smbfs` changes its prompt on a future macOS] → the prompt is matched loosely, a timeout ends the attempt with the system's last line, and the `osascript` fallback is documented.
- [gvfs FUSE paths are slow and its rename semantics differ] → scans already limit reads per drive and treat gvfs as a network file system for deletes; a note in the page suggests the system mount for large libraries.
- [Windows error 1219 when Explorer is already connected as another user] → mapped to a sentence that says what to do.
- [Docker Desktop keeps the password in the volume options] → stated by the installer and the README; the Linux Docker path keeps it in a root-only file instead.
- [A CIFS volume fails the container start while the NAS is off] → `--restart unless-stopped` retries; the README says the Linux host mount avoids this.
- [The password is in process memory while not remembered] → the same trust boundary as the running app; it is dropped when the share is forgotten or the app quits.

## Migration Plan

Additive. A schema migration adds `network_share`; libraries without shares are unaffected. Existing UNC library folders keep working as before and are not linked to a share row unless the user connects that share through the page. Rollback: an older version ignores the table (the backup taken before the migration remains).

## Open Questions

1. **Scoped `unsafe` on Windows.** `WNetAddConnection2W` / `WNetCancelConnection2W` in a small module with `#[allow(unsafe_code)]` and `SAFETY:` comments, like the MediaInfo FFI, would replace PowerShell with a faster call and typed errors. Keep the PowerShell approach (no new exception), or allow this second exception?
2. **macOS mount folder.** `~/CineVault Shares/<server>/<share>` is visible in the home folder. Is that acceptable, or would the owner prefer the `osascript` route that mounts in `/Volumes` like Finder, at the cost of a possible system dialog and `-1` name suffixes?
3. **Linux Docker.** The owner's brief named a CIFS volume for containers. The design uses a host mount with a root-only credentials file for Linux `--docker` (password never in Docker, container starts while the NAS is off) and the CIFS volume only for Docker Desktop. Confirm, or use the CIFS volume everywhere for one code path.
4. **SMB version.** `vers=3.0` is the installer default. Some older NAS units need `2.1` or `1.0`; should the installer retry with `2.1` automatically, or ask?
