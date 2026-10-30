# Spec Delta

## Purpose

Runs CineVault on the machine that holds the media files and serves the same interface and the same actions to a browser on another machine, so a server with no desktop session needs no mounted drives and the person using it needs no desktop app.

## ADDED Requirements

### Requirement: Headless server
CineVault SHALL provide a server program that runs on Linux without a display, a desktop session or a window, opens the library of its data folder and serves the interface as static files with the command surface behind it. It SHALL bind its port before it starts the library's background work, so a health check answers while the library is opening, and SHALL answer 503 until the library is ready. It SHALL log to standard output and standard error, SHALL exit with a non-zero code when the data folder cannot be opened, and SHALL handle termination requests by pausing running jobs and removing its owner file so the next start resumes them.

#### Scenario: Start on a server with no display
- **WHEN** the server program is started on a Linux machine with no `DISPLAY` and a data folder that holds a library
- **THEN** it prints the address it listens on, `/healthz` answers 503 while the library opens and 200 once it is ready, and `GET /` answers with the interface

#### Scenario: Library cannot be opened
- **WHEN** the data folder holds a library from a newer schema
- **THEN** the program prints the reason, answers 503, and exits with a non-zero code

#### Scenario: Clean stop
- **WHEN** the process is asked to terminate while a scan runs
- **THEN** the job is paused, the owner file is removed, and the next start resumes the scan

### Requirement: Full command parity
Every command the desktop application offers SHALL be available through the server with the same inputs, outputs, errors and side effects, except the commands the "Desktop-only commands" requirement names. The server's dispatcher SHALL be a table separate from the desktop registry, and a check SHALL fail when a command is missing from either table or an argument name differs between them. Commands that complete only after long work (a cross-drive move, a tool download, a first-run analysis) SHALL return an operation id and stream their progress instead of holding one request open.

#### Scenario: A page that is not the library
- **WHEN** a signed-in user opens Settings, a movie page, the artwork picker, the rename preview and the History page in a browser
- **THEN** each page loads its data and its actions work as they do in the desktop app

#### Scenario: A command missing from the server
- **WHEN** a command is registered for the desktop app and not for the server
- **THEN** the parity check fails and names the missing command

#### Scenario: An argument renamed on one side
- **WHEN** the server expects `stop_jobs` and the desktop registry says `stopJobs`
- **THEN** the parity check fails and names the command and the argument

#### Scenario: A long move behind a proxy
- **WHEN** a move of a large title starts from a browser behind a reverse proxy with a 60-second read timeout
- **THEN** the start call answers with an operation id before the timeout and progress continues on the stream

### Requirement: First-run welcome
On a start with no password set, the server SHALL serve a welcome page that asks for a new password of at least 12 characters and a confirmation, and nothing else. There SHALL be no default password and no setup token. The welcome pages SHALL be available only until a password exists and only for 30 minutes after the first start; the first browser to open them SHALL claim the setup with a short-lived setup cookie, and a second browser SHALL be refused. The welcome page SHALL say plainly that setup must be finished before the server is made reachable from the internet. After the window, only the server's own password command SHALL be able to set or reset the password.

#### Scenario: First visit
- **WHEN** the server starts on a new data folder and a browser opens the address
- **THEN** the welcome page asks for a password and its confirmation, and nothing else is served

#### Scenario: Second browser during setup
- **WHEN** a second browser opens the welcome page while the first one holds the setup
- **THEN** it is told that setup is in progress on another browser and no password can be set from it

#### Scenario: Setup window closed
- **WHEN** 30 minutes pass before a password is set
- **THEN** the welcome page is replaced by a page that names the server's password command, and no password can be set from a browser

### Requirement: Sign-in
The server SHALL have exactly one user. Before any page, API call, image, media file or stream is served, except the welcome pages, the sign-in page, the health endpoint and the sign-in and setup requests themselves, the request SHALL carry a valid session. A session cookie SHALL be `HttpOnly` and `SameSite=Strict`, SHALL be `Secure` when the effective scheme is HTTPS, SHALL expire after 30 days of no use or 90 days in total, and SHALL be one of at most 32 live sessions, the oldest being evicted first. Signing out SHALL end the session on the server and close that session's streams. Five failed sign-ins from one address within one minute SHALL be answered with "Too many attempts. Wait a minute and try again." and no password check, with a global ceiling that a proxy address cannot use to lock out every client. A request without a session SHALL be refused with the typed value `{code: "signed_out"}` and no library data.

#### Scenario: Signed out
- **WHEN** a browser without a session asks for `/library`, `/api/invoke`, `/img/<key>/grid`, `/api/media/1/0` or `/api/stream`
- **THEN** each is refused as signed out and no bytes of the page, command, image or media file are sent

#### Scenario: Wrong password
- **WHEN** the user enters a wrong password five times in a minute
- **THEN** the sixth attempt is refused without checking the password, and the message names the wait

#### Scenario: Sign out
- **WHEN** the user signs out and the browser sends the old cookie
- **THEN** the server answers as signed out, and an open stream from that session is closed

#### Scenario: Session cap
- **WHEN** 33 sessions are created
- **THEN** the oldest is ended and the newest 32 stay

### Requirement: Write protection
Every command invocation SHALL require the session's CSRF token and an `Origin` that matches the effective host, on reads as well as writes. A missing or mismatched token SHALL be refused with 403 and no change. The token SHALL be issued with the session and rotated on sign-in.

#### Scenario: Missing CSRF token
- **WHEN** a signed-in browser sends an invoke without the CSRF token
- **THEN** the answer is 403 and the command does not run

#### Scenario: Foreign origin
- **WHEN** a request arrives with an `Origin` for another host
- **THEN** the answer is 403 and no command runs

### Requirement: Network exposure
The server SHALL listen on `127.0.0.1` unless the operator asks for another address. It SHALL print the address and whether it is reachable beyond the machine. `CINEVAULT_WEB_TRUST_PROXY` SHALL name the proxy addresses (CIDRs); only requests from those addresses SHALL be allowed to set the effective scheme from `X-Forwarded-Proto`, the client address from `X-Forwarded-For` and the host from `X-Forwarded-Host`. Without a trusted proxy the socket peer and the connection's own scheme SHALL be used. The server SHALL NOT redirect HTTP to HTTPS itself; the documentation SHALL describe putting it behind a reverse proxy for HTTPS. The sign-in and welcome pages SHALL warn when the effective connection is plain HTTP and the address is not loopback.

#### Scenario: Default address
- **WHEN** the server starts with no address configured
- **THEN** it listens only on `127.0.0.1`

#### Scenario: Plain HTTP on the LAN
- **WHEN** the server is bound to a LAN address and opened over HTTP without a trusted proxy
- **THEN** the welcome and sign-in pages say the connection is not encrypted and point at the documentation

#### Scenario: Trusted proxy
- **WHEN** `CINEVAULT_WEB_TRUST_PROXY=10.0.0.0/8` is set and a request from `10.0.0.2` carries `X-Forwarded-Proto: https`
- **THEN** the session cookie is `Secure` and the request is treated as HTTPS

#### Scenario: Spoofed proxy header
- **WHEN** a request from an address outside the trusted range carries `X-Forwarded-Proto: https`
- **THEN** the header is ignored and the connection is treated as plain HTTP

### Requirement: Request limits and path safety
The server SHALL cap a command request at 1 MiB, an artwork upload at 32 MiB plus multipart overhead, and a request body at 33 MiB overall, and SHALL time out a request that sends no body bytes for 30 seconds. A stream connection SHALL NOT be closed by that body timeout; it SHALL be kept alive by a comment or ping at least every 20 seconds. Static file serving SHALL refuse any path that resolves outside the interface folder. The folder browser SHALL accept only absolute paths and SHALL refuse a path outside the configured roots. The image route SHALL accept only a parsed 32-character key and a size word. The media route SHALL accept only an item and part number, SHALL canonicalise the file, SHALL require it to be inside a library folder using a component-wise comparison, and SHALL re-resolve it on every request. No route SHALL answer with the contents of an arbitrary file.

#### Scenario: Path traversal
- **WHEN** a request asks for `/../../etc/passwd` or an encoded form of it
- **THEN** the server answers 404 and no file outside the interface folder is read

#### Scenario: Folder browser escape
- **WHEN** the folder browser is asked for a path outside the allowed roots
- **THEN** it refuses with "That folder is outside the folders this server may browse." and lists nothing

#### Scenario: Symlink planted after the scan
- **WHEN** a symbolic link inside a library folder points outside it and a media request follows it
- **THEN** the request is refused with 404 and no bytes are sent

#### Scenario: Idle stream
- **WHEN** a stream connection is open for two minutes with no messages
- **THEN** it stays open and the browser reconnects normally after a network drop

### Requirement: Browser shell
The interface served to a browser SHALL show the same sidebar, pages, pickers, search, selection, keyboard model and themes as the desktop app. It SHALL NOT show window controls, a tray, a taskbar or dock, and it SHALL use the browser's own back and forward history. The document title SHALL name the page and the app. An address that names a page SHALL open that page after sign-in. `index.html` SHALL NOT be cached; hashed assets SHALL be immutable. A build id SHALL be checked on load, and a mismatch SHALL offer "CineVault was updated, reload".

#### Scenario: Deep link
- **WHEN** a signed-in user opens `/movies/42`
- **THEN** the Movie page of that title opens, and the browser's back button returns to the previous page

#### Scenario: No window controls
- **WHEN** any page is open in a browser
- **THEN** no minimize, maximize or close control and no taskbar progress is shown

#### Scenario: Stale bundle after an update
- **WHEN** the server is updated while a tab is open and the tab loads a page again
- **THEN** the build id differs and the page offers to reload

### Requirement: Images
In a browser, images SHALL be served by the server at `/img/<key>/<size>` for the same key and size words as the desktop image protocol, with the same content types, ETag and 304 answer, the same 404 for an unknown key, the same 405 for methods other than GET and HEAD, the same `full` original and hero stand-in rules, and the same "largest cached size with `no-store`" stand-in for a size that cannot be made yet. The route SHALL require a session. Its cache header SHALL be `private, max-age=31536000, immutable` and it SHALL NOT send a cross-origin allowance. Only a parsed 32-character key and a size word SHALL reach the file system.

#### Scenario: A repeated request
- **WHEN** the browser asks again for a grid thumbnail with the ETag it holds
- **THEN** the server answers 304 and sends no image

#### Scenario: Signed out image request
- **WHEN** an image is requested without a session
- **THEN** it is refused as signed out and no image is sent

#### Scenario: Unknown key
- **WHEN** a browser asks for `/img/not-a-key/grid`
- **THEN** the answer is 404 and no file is read

#### Scenario: Wrong method
- **WHEN** a browser sends POST to an image address
- **THEN** the answer is 405

### Requirement: Streams
Job progress, notices and every stream a page opens (scrape candidates, artwork candidates, torrent results, tool installs and update downloads) SHALL reach the browser over one Server-Sent Events connection per page. The page SHALL mint a client id and send it on the stream and on every command, so a stream is bound to the page that opened it and not to the shared session cookie. Each message SHALL carry a monotonic id, an event kind and the stream id. A dropped connection SHALL reconnect within five seconds and send `Last-Event-ID`; the server SHALL replay from a bounded per-client buffer. When the buffer overflows, the server SHALL send a `reset` and the page SHALL reload that stream's or job's state before continuing, so no item is doubled or lost. Signing out, changing the password and session expiry SHALL close that session's connections.

#### Scenario: Dropped connection during a scrape
- **WHEN** the connection drops while a scrape is streaming candidates and reconnects
- **THEN** the page receives the candidates it missed and the result is the same as without the drop

#### Scenario: Buffer overflow
- **WHEN** a page misses more events than the buffer holds
- **THEN** it receives `reset`, reloads the stream state and continues without duplicate items

#### Scenario: Idle
- **WHEN** no job is running and no stream is open
- **THEN** the connection carries no progress messages

#### Scenario: Sign-out with an open stream
- **WHEN** the user signs out while a scrape stream is open
- **THEN** the connection is closed and no further candidates are sent

### Requirement: Server folder browser
Where the desktop app opens the system folder picker or accepts a folder dropped on the window, the browser SHALL show a folder browser over the server's file system: it lists folders with their names, navigates by opening one or by typing an absolute path, shows the current path in full, can make a folder, and can choose the current folder. The roots SHALL default to the data folder and the existing library folders; `/` SHALL require an explicit `CINEVAULT_WEB_ROOTS=/`. It SHALL show no file contents and SHALL list only folders. It SHALL refuse a path outside the roots with the message in "Request limits and path safety". Making a folder SHALL require the CSRF token, SHALL refuse a dot-directory and a path outside the roots, and SHALL have no undo beyond deleting the empty folder. A folder dropped on the page SHALL NOT be accepted as a path, because a browser does not provide one; the page SHALL say "Use Add folder…".

#### Scenario: Add a library folder
- **WHEN** a signed-in user chooses "Add folder…" in Settings › Library folders
- **THEN** the folder browser opens at a root, and choosing `/media/movies` adds it as a library folder

#### Scenario: Typed path
- **WHEN** the user types `/media/movies` and confirms
- **THEN** the browser lists that folder, or says it is not a folder

#### Scenario: Make a folder
- **WHEN** the user makes `Season 1` inside `/media/tv/Example Show`
- **THEN** the folder exists, and making `.ssh` or a path outside the roots is refused

#### Scenario: Dropped folder
- **WHEN** the user drops a folder on the first-run page
- **THEN** the page says "Use Add folder…" and no path is added

### Requirement: Artwork uploads
Where the desktop app accepts an image dropped from the desktop, the browser SHALL accept a file chosen or dropped on the picker. The upload SHALL accept PNG, JPEG, WebP and AVIF only, refuse a file over 32 MiB with "That image is larger than 32 MiB.", verify the file is an image by its content rather than its name, and add it to the preview rather than writing it. Pressing "Use this" SHALL write it through the same artwork path as a downloaded candidate, including the undo entry and the deleted-files destination. The request body SHALL be staged in the data folder only for validation; the final file SHALL be written beside its target so the replace is atomic on a different volume. A temporary file left by an interrupted upload SHALL be cleaned on the next start.

#### Scenario: Upload a poster
- **WHEN** the user chooses a 3000×4500 JPEG for the poster slot and presses "Use this"
- **THEN** it is written as the poster with the configured file name, the previous poster goes to the deleted-files destination, and Undo restores it

#### Scenario: Not an image
- **WHEN** a file named `poster.png` holds a zip archive
- **THEN** the upload is refused with "That file is not an image."

#### Scenario: Cross-volume write
- **WHEN** the data folder and the media folder are different volumes and the server dies during the write
- **THEN** the old poster is still in place, and the next start removes the temporary file

### Requirement: Browser playback
Play in a browser SHALL offer "Play in browser", "Copy stream link" and, when Kodi is configured, "Open in Kodi". The browser player SHALL use a media route that answers range requests, sends a video content type from an allowlist and `application/octet-stream` for anything else, sets `X-Content-Type-Options: nosniff` and a sandboxing CSP, and refuses a file outside the library folders. A download route SHALL always answer `Content-Disposition: attachment`. A stacked title SHALL be offered as its parts in order. A disc folder SHALL open its main index file when it is readable, else Play SHALL be off with a sentence. When the browser cannot decode the file, the page SHALL say so and offer the stream link and the download instead of a failing player. No player SHALL be started on the server.

#### Scenario: Play a movie
- **WHEN** the user presses Play in a browser on a movie the browser can decode
- **THEN** the browser's own player starts, seeking works, and no process starts on the server

#### Scenario: A format the browser cannot play
- **WHEN** the file is a DTS-in-MKV the browser cannot decode
- **THEN** the page says the browser cannot play it and offers the stream link and the download

#### Scenario: Stacked movie
- **WHEN** a movie has two parts
- **THEN** the browser receives both parts in order and plays them one after the other

#### Scenario: File moved away
- **WHEN** a movie's video file was deleted outside CineVault and the user presses Play in a browser
- **THEN** Play is off and says the file is not on disk

#### Scenario: Outside the library
- **WHEN** a media address names a file outside every library folder
- **THEN** the server answers 404 and sends no bytes

### Requirement: One writer
The server SHALL hold the data folder for as long as it runs, through a portable owner file in the data folder that records its role, process, host, start time, protocol and a heartbeat no older than 30 seconds. The desktop app SHALL refuse to open a data folder whose owner is a fresh server, and SHALL say so in the window. The `cinevault` command line SHALL try the server's local channel; when the owner file says a server holds the folder and the channel is unreachable, it SHALL exit with code 2 and a sentence naming the server, and SHALL NOT open the library itself. A second server on the same data folder SHALL refuse to start and say why. A stale owner SHALL be taken over after 30 seconds. The documentation SHALL say the data folder must be on a local filesystem, not an SMB or NFS share.

#### Scenario: Desktop app on the same folder
- **WHEN** the desktop app is started with the data folder the server holds
- **THEN** it says the library is in use by the server and opens no library

#### Scenario: CLI while the server runs
- **WHEN** `cinevault movie --rename-all` runs while the server holds the data folder and its channel is reachable
- **THEN** the server runs the batch and the command line prints its report

#### Scenario: CLI on the host, server in a container
- **WHEN** the host CLI sees the server's owner file but cannot reach its socket
- **THEN** it exits with code 2, names the server and opens no library

#### Scenario: Crashed server
- **WHEN** the server crashed and left an owner file whose heartbeat is 31 seconds old
- **THEN** the desktop app takes the folder over

### Requirement: Server settings
Settings SHALL gain a Server section, shown only when the page is served by the server, with the address and port (read-only, with the reason in a hint), the running version, the session lifetime, "Change password", "Sign out every session", "Allow permanent delete", the Kodi connection (host, port, optional credentials, path mapping) and the trusted proxy state. Changing the password SHALL require the current password and SHALL end every other session. Permanent delete SHALL be off by default.

#### Scenario: Change the password
- **WHEN** the user changes the password from Settings › Server
- **THEN** the current session stays signed in, every other session is signed out, and the next sign-in needs the new password

#### Scenario: Desktop app
- **WHEN** Settings is opened in the desktop app
- **THEN** no Server section is shown

#### Scenario: Permanent delete off
- **WHEN** permanent delete has not been enabled and a browser asks to erase a title
- **THEN** the request is refused and the UI offers the configured deleted-files destination instead

### Requirement: Permanent delete from a browser
When permanent delete is enabled, the server SHALL mint a one-time intent bound to the exact item IDs, valid for 60 seconds. The erase request SHALL carry that intent and the current password; a missing, reused, expired or mismatched intent, or a wrong password, SHALL be refused before any file is touched. The bin/trash delete SHALL keep its no-dialog behaviour.

#### Scenario: Erase with an intent
- **WHEN** the user confirms permanent delete and sends the minted intent with the current password
- **THEN** the listed files are erased and a History entry records every path

#### Scenario: Replayed intent
- **WHEN** the same intent is sent twice
- **THEN** the second request is refused and no file is touched

#### Scenario: Bin delete
- **WHEN** the user deletes a title to the configured destination
- **THEN** no confirmation dialog is shown and the title can be restored

### Requirement: Health
The server SHALL answer `GET /healthz` without a session with 200 and `{"status":"ok"}` when the library is open, and 503 with the reason while it is starting or after the library became unreadable. When the server's provider-key master key is not available, the answer SHALL say `"keys":"locked"` and the server SHALL still be healthy. The answer SHALL hold no library data.

#### Scenario: Container health check
- **WHEN** the container's health check asks `/healthz` after the library opened
- **THEN** the answer is 200 and the container is healthy

#### Scenario: Starting
- **WHEN** `/healthz` is asked before the library is open
- **THEN** the answer is 503

#### Scenario: Keys locked
- **WHEN** the library is open but no master key is configured
- **THEN** the answer is 200 with `"keys":"locked"`, and Settings says how to set it

### Requirement: Packaging
A Linux release SHALL publish a container image for amd64 and arm64 under `ghcr.io/<owner>/cinevault`, a `cinevault-server` `.deb` and `.rpm` (distinct from the desktop `cinevault` package), and a tarball, all built from the same source and carrying the same version. The image SHALL run as a non-root user, SHALL accept `PUID`/`PGID`, SHALL mount the data folder and the library folders read-write, SHALL declare a health check and SHALL default the deleted-files destination to `.deletedByCineVault` because the freedesktop trash is not writable on a bind mount. The packages SHALL contain the server, the `cinevault` command line and the interface assets. The repository SHALL hold a Docker Compose example and a systemd unit example. Setup SHALL write and remove a probe file in every library folder and say "This folder is not writable by the server" when it cannot.

#### Scenario: Compose with two volumes
- **WHEN** the image is started with the data folder and `/media/movies` mounted read-write
- **THEN** the library is served, `/media/movies` can be added, and a rename in it succeeds

#### Scenario: Non-root
- **WHEN** the image starts with its default user
- **THEN** no process in it runs as root, and the mounted folders are writable by its user

#### Scenario: Read-only library folder
- **WHEN** a library folder cannot be written
- **THEN** setup says "This folder is not writable by the server" before the scan starts

#### Scenario: arm64 tools
- **WHEN** the arm64 image runs a scan and a media info read
- **THEN** MediaInfo and FFmpeg are available for `linux-aarch64`, or the release does not offer arm64

### Requirement: Server updates
The server SHALL NOT install the desktop update. Settings › Updates in a browser SHALL say that the server is updated by pulling a new image or installing a new package, SHALL show the running version, and SHALL link to the release notes. `index.html` SHALL be served `no-store`, hashed assets immutably, and a build-id mismatch SHALL offer a reload. The in-app installer and the signed update manifest SHALL remain desktop-only.

#### Scenario: Updates in a browser
- **WHEN** the user opens Settings › Updates in a browser
- **THEN** no Install button is shown, and the page names the image or package and the running version

#### Scenario: Rollback
- **WHEN** a newer server migrated the library and an older server is started on it
- **THEN** it refuses the library and says a same-or-newer version is needed

### Requirement: Desktop-only commands
A command that cannot work in a browser (starting a local player, opening a folder or a web page on the client, changing the window, the taskbar mirror, installing the desktop update, the taskbar/tray controls) SHALL answer `{code: "not_applicable", message}` where the message says what replaces it or why the browser has no equivalent, and SHALL NOT fail silently or leave a spinner. The interface SHALL hide or disable the control that would call it. The desktop-only list SHALL include `open_folder`, `pick_folder`, `pick_files`, `play_external`, `open_rating_page`, `open_logs`, `torrent_open_page`, `torrent_open_magnet`, `torrent_open_torrent_file`, `torrent_open_indexer_manager`, the window and taskbar commands and `install_update`.

#### Scenario: Open folder
- **WHEN** a browser asks to open a title's folder
- **THEN** the answer is `not_applicable` with "The folder is on the server.", and the button is not shown

#### Scenario: Torrent hand-off
- **WHEN** a browser sends a release to the torrent app
- **THEN** the magnet opens through the browser and the toast offers "Copy magnet link"

### Requirement: Multiple clients
Every job SHALL record the client and session that started it. The interface SHALL show "Started from another session" on a job another session started, and cancelling or pausing it from a different session SHALL ask first. Shared operation slots (a move, a tool download, an update install) SHALL answer a busy error keyed by operation id instead of replacing each other's work.

#### Scenario: Two tabs
- **WHEN** a scan is started in one tab and the second tab lists the jobs
- **THEN** the job says it was started from another session, and cancelling it from the second tab asks first

#### Scenario: Two moves
- **WHEN** a second move starts while one runs
- **THEN** it is refused as busy and the first move is untouched

### Requirement: Kodi
Settings SHALL offer an optional Kodi connection (host, port, optional credentials and a path mapping), off by default. When it is configured, a finished Scan, Scrape or Rename batch MAY ask Kodi to update its library, and the browser MAY offer "Open in Kodi" for a title. A failure SHALL say Kodi is unreachable and SHALL NOT fail the batch.

#### Scenario: Library update after a scrape
- **WHEN** a scrape finishes and Kodi is configured
- **THEN** the server calls Kodi's library scan and the browser shows whether it was asked

#### Scenario: Kodi unreachable
- **WHEN** Kodi does not answer
- **THEN** the toast says Kodi is unreachable and the scrape result is kept
