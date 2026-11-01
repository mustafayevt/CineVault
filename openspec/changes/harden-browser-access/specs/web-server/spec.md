## MODIFIED Requirements

### Requirement: First-run welcome
On a start with no password set, the headless server SHALL serve a welcome page that asks for a new password of at least 12 characters and a confirmation, and nothing else. There SHALL be no default password and no setup token. The welcome pages SHALL be available for as long as no password exists, but only to browsers from the home network as the "Forwarded requests are not from the home network" requirement judges it: a loopback, private (`10/8`, `172.16/12`, `192.168/16`, `fc00::/7`), link-local (`169.254/16`, `fe80::/10`) or carrier-grade NAT (`100.64/10`) client address, taken from the socket peer, or from `X-Forwarded-For` only when the peer is a trusted proxy, and never for a request that carries a forwarding header from a peer that is not a trusted proxy. A browser from any other address SHALL get a page that says "Finish setup from your home network." and names the server's password command, and SHALL NOT be able to set a password. The first home-network browser to open the welcome page SHALL claim the setup with a setup cookie; a second browser SHALL be refused while the claim is in use, and a claim unused for 30 minutes SHALL lapse. The welcome page SHALL say plainly that setup must be finished before the server is made reachable from the internet. The desktop app's Browser access SHALL never show the welcome page; its password is set in the app's Settings.

#### Scenario: First visit
- **WHEN** the server starts on a new data folder and a browser on the same network opens the address
- **THEN** the welcome page asks for a password and its confirmation, and nothing else is served

#### Scenario: Second browser during setup
- **WHEN** a second browser opens the welcome page while the first one holds the setup
- **THEN** it is told that setup is in progress on another browser and no password can be set from it

#### Scenario: Setup window closed
- **WHEN** a password exists and a browser opens the welcome page
- **THEN** it is sent to the sign-in page, and the password can be changed only from a signed-in session, with a reset PIN or with the server's password command

#### Scenario: Setup from a public address
- **WHEN** no password exists and a browser at a public address opens the server
- **THEN** it is told to finish setup from the home network and no password can be set from it

#### Scenario: Welcome after a long time
- **WHEN** the server has run for two hours without a password and a home-network browser opens it
- **THEN** the welcome page is served and the password can be set

#### Scenario: Abandoned claim
- **WHEN** the browser that claimed setup is closed and another home-network browser opens the welcome page 31 minutes later
- **THEN** the second browser claims the setup and can set the password

#### Scenario: Desktop Browser access
- **WHEN** a browser opens `/setup` on the desktop app's Browser access
- **THEN** it is sent to the sign-in page

#### Scenario: Setup through an untrusted local proxy
- **WHEN** no password exists, `CINEVAULT_WEB_TRUST_PROXY` is unset, and a request arrives from `127.0.0.1` with `X-Forwarded-For: 203.0.113.7`
- **THEN** it is told to finish setup from the home network and no password can be set from it

### Requirement: Sign-in
The server SHALL have exactly one user. Before any page, API call, image, media file or stream is served, except the welcome pages, the sign-in page, the health endpoint and the sign-in and setup requests themselves, the request SHALL carry a valid session. A session cookie SHALL be `HttpOnly` and `SameSite=Strict`, SHALL be `Secure` when the effective scheme is HTTPS, SHALL expire after 30 days of no use or 90 days in total, and SHALL be one of at most 32 live sessions, the oldest being evicted first. Signing out SHALL end the session on the server and close that session's streams. Five failed sign-ins from one address within one minute SHALL be answered with "Too many attempts. Wait a minute and try again." and no password check. A global ceiling of 100 failed sign-ins a minute SHALL refuse further sign-ins only from addresses outside the home network, so a flood cannot lock out a home-network browser, and SHALL never end a signed-in session. A request without a session SHALL be refused with the typed value `{code: "signed_out"}` and no library data.

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

#### Scenario: Flood from the internet
- **WHEN** 100 wrong passwords arrive within a minute from many public addresses and the owner then signs in from 192.168.1.30 with the right password
- **THEN** the owner is signed in, a new attempt from another public address is refused as too many attempts, and an already signed-in browser keeps working

### Requirement: Write protection
Every command invocation SHALL require the session's CSRF token, on reads as well as writes. A request that carries an `Origin` SHALL be refused unless it matches the effective host; a request without an `Origin` SHALL be judged by the CSRF token alone. A missing or mismatched token, or a mismatched `Origin`, SHALL be refused with 403 and no change. The token SHALL be issued with the session and rotated on sign-in.

#### Scenario: Missing CSRF token
- **WHEN** a signed-in browser sends an invoke without the CSRF token
- **THEN** the answer is 403 and the command does not run

#### Scenario: Foreign origin
- **WHEN** a request arrives with an `Origin` for another host
- **THEN** the answer is 403 and no command runs

#### Scenario: No Origin header
- **WHEN** a client with a valid session cookie and CSRF token sends an invoke without an `Origin`
- **THEN** the command runs, and the same request without the token is refused with 403

### Requirement: Desktop-only commands
A command that cannot work in a browser (starting a local player, opening a folder or a web page on the client, changing the window, the taskbar mirror, installing the desktop update, the taskbar/tray controls) or that would let a browser choose a program the host runs SHALL answer `{code: "not_applicable", message}` where the message says what replaces it or why the browser has no equivalent, and SHALL NOT fail silently or leave a spinner. The interface SHALL hide or disable the control that would call it. The desktop-only list SHALL include `open_folder`, `pick_folder`, `pick_files`, `play_external`, `open_rating_page`, `open_logs`, `torrent_open_page`, `torrent_open_magnet`, `torrent_open_torrent_file`, `torrent_open_indexer_manager`, `set_tool_path`, the window and taskbar commands and `install_update`.

#### Scenario: Open folder
- **WHEN** a browser asks to open a title's folder
- **THEN** the answer is `not_applicable` with "The folder is on the server.", and the button is not shown

#### Scenario: Torrent hand-off
- **WHEN** a browser sends a release to the torrent app
- **THEN** the magnet opens through the browser and the toast offers "Copy magnet link"

#### Scenario: Tool path from a browser
- **WHEN** a browser asks to use `\\files\share\ffmpeg.exe` as FFmpeg
- **THEN** the answer is `not_applicable` with "A tool's path is set on the computer that runs CineVault.", the file is not run, and the tool's path is unchanged

### Requirement: Home network only
While "Only devices on my home network" is on (the default), the desktop app's Browser access SHALL refuse every request whose peer address is not a loopback, private, link-local or carrier-grade NAT address, and every request that carries a forwarding header, with 403 and "Browser access is limited to your home network.".

#### Scenario: Address outside the home network
- **WHEN** the option is on and a request arrives from a public address
- **THEN** it is refused with 403 and no page or command is served

#### Scenario: Option off
- **WHEN** the option is off and a request arrives from a public address
- **THEN** the sign-in page is served and every other sign-in rule still applies

#### Scenario: Tunnel on the same computer
- **WHEN** the option is on and a tunnel program on the same computer forwards a visitor's request from `127.0.0.1` with `X-Forwarded-For: 198.51.100.4`
- **THEN** it is refused with 403 and no page or command is served

## ADDED Requirements

### Requirement: Forwarded requests are not from the home network
A request that carries a forwarding header (`Forwarded`, `X-Forwarded-For`, `X-Forwarded-Host`, `X-Forwarded-Proto`, `X-Real-IP`, `CF-Connecting-IP` or `True-Client-IP`) from a peer that is not a trusted proxy SHALL be judged outside the home network by the welcome pages, the PIN reset, the sign-in ceiling and Browser access's home-network option. The first such request in a run SHALL be logged as a warning that names `CINEVAULT_WEB_TRUST_PROXY`.

#### Scenario: Reset through an untrusted proxy
- **WHEN** a request from `127.0.0.1` carries `X-Real-IP: 203.0.113.7` and asks for a password reset, and no trusted proxy is set
- **THEN** it is refused with the home-network sentence and no PIN file is written

#### Scenario: Trusted proxy
- **WHEN** `CINEVAULT_WEB_TRUST_PROXY=127.0.0.1` is set and a request from `127.0.0.1` carries `X-Forwarded-For: 192.168.1.30`
- **THEN** the request is judged from the home network

#### Scenario: One warning
- **WHEN** three requests with `X-Forwarded-For` arrive from an untrusted peer
- **THEN** the log holds one warning naming `CINEVAULT_WEB_TRUST_PROXY`

#### Scenario: Port mapping without headers
- **WHEN** a request without forwarding headers arrives from the container bridge address `172.17.0.1`
- **THEN** it is judged from the home network

### Requirement: Host settings refused from a browser
A browser SHALL NOT change the update channel, the update signing key, a tool's path or the video player settings; `set_setting` for one of them SHALL be refused with "This setting is changed on the computer that runs CineVault." and change nothing, and resetting settings from a browser SHALL leave them as they are. The custom trash folder and the collection data folder SHALL be accepted from a browser only when empty or inside the server's folder roots.

#### Scenario: Update channel from a browser
- **WHEN** a signed-in browser sets `general.update-channel` to `https://updates.example.net/manifest.json`
- **THEN** the request is refused with the sentence and the desktop keeps reading its built-in channel

#### Scenario: Player command from a browser
- **WHEN** a signed-in browser sets `player.custom-command`
- **THEN** the request is refused and Play in the desktop app starts the player chosen at the computer

#### Scenario: Trash folder outside the roots
- **WHEN** a browser sets the custom trash folder to `/etc/cron.d` on a server whose roots are `/media` and the data folder
- **THEN** the request is refused and deleted files keep going where they went

#### Scenario: Trash folder inside the roots
- **WHEN** a browser sets the custom trash folder to `/media/movies/.trash`
- **THEN** the setting is saved

#### Scenario: Reset to defaults from a browser
- **WHEN** a browser resets the General settings, including a custom update channel set at the computer
- **THEN** the other General settings return to their defaults and the update channel is unchanged

### Requirement: No framing
Every response of the server and of the desktop app's Browser access SHALL carry `X-Frame-Options: DENY` and a `Content-Security-Policy` that includes `frame-ancestors 'none'`, so no other site can show a CineVault page in a frame.

#### Scenario: Page in a frame
- **WHEN** another site embeds the server's address in a frame
- **THEN** the browser refuses to show it, because the response forbids framing

#### Scenario: Media response
- **WHEN** a media file is requested
- **THEN** its one `Content-Security-Policy` keeps the sandbox and adds `frame-ancestors 'none'`

### Requirement: Sessions kept across restarts
Sessions SHALL be kept in `server-sessions.json` in the data folder, owner-only and written atomically, holding a hash of each session id and never the id itself, so a restart of the server or of the desktop app keeps browsers signed in within the usual lifetimes. Signing out, "Sign out every session", a password change by any means, and turning Browser access off or changing its port SHALL end the sessions they end today.

#### Scenario: Server restart
- **WHEN** the server is restarted while a browser is signed in
- **THEN** the browser's next request is served without signing in again

#### Scenario: Desktop app update
- **WHEN** the desktop app quits to install an update while Browser access is on and a phone is signed in
- **THEN** after the app starts again the phone is still signed in

#### Scenario: Password changed on the command line
- **WHEN** `cinevault-server passwd` sets a new password while the server is stopped and the server is started again
- **THEN** every earlier session is answered as signed out

#### Scenario: Browser access switched off
- **WHEN** Browser access is turned off and on again
- **THEN** every browser has to sign in again

#### Scenario: Damaged sessions file
- **WHEN** `server-sessions.json` cannot be read at start
- **THEN** the server starts with no sessions, keeps the unreadable file aside and logs why

#### Scenario: No session id on disk
- **WHEN** `server-sessions.json` is read after a sign-in
- **THEN** it does not contain the cookie value of any session and only its owner can read it
