## ADDED Requirements

### Requirement: Browser access from the desktop app
With Browser access on, the desktop app SHALL serve the browser interface from its own process over the library it has open, with the headless server's routes, sign-in, sessions, CSRF and `Origin` checks, rate limits, media session binding, folder roots and commands. It SHALL open no second library and take no second owner file. Provider keys SHALL stay in the OS credential store and be used without being sent. The command line SHALL keep forwarding batches to the app.

#### Scenario: Switch on
- **WHEN** the user sets a password and turns Browser access on with port 8787
- **THEN** `http://<the computer's address>:8787/` answers with the sign-in page, and signing in shows the library the app has open

#### Scenario: Provider key from a browser
- **WHEN** a signed-in browser scrapes a title in the app's Browser access
- **THEN** the scrape uses the key from the app's credential store, and the Providers section in the browser shows the key as entered without its value

#### Scenario: Command line while Browser access runs
- **WHEN** `cinevault movie --rename-all` runs while the app serves Browser access
- **THEN** the batch runs in the app as before and both the window and the browser see the renamed titles

#### Scenario: Desktop-only commands from a browser
- **WHEN** a browser invokes a Browser access command (turn on or off, set the password, change the port)
- **THEN** it is answered as not applicable and nothing changes

### Requirement: Browser access port
The desktop app SHALL listen on all IPv4 addresses on the chosen port (8787 by default) only while Browser access is on, and SHALL NOT start without a password. A port another program holds SHALL leave the server stopped with a sentence naming a free port. Turning Browser access off or changing the port SHALL end every browser session, close every stream and close the old port.

#### Scenario: Port in use
- **WHEN** another program listens on the chosen port and Browser access is turned on
- **THEN** nothing is started, the section says "Port 8787 is in use by another program. Choose another port, such as 8788.", and choosing a free port starts it

#### Scenario: Switch off
- **WHEN** Browser access is turned off while a browser is signed in and playing a title
- **THEN** the port no longer accepts connections, the playback stops, and the browser's next request is answered as signed out

#### Scenario: Restart with a new port
- **WHEN** the port is changed from 8787 to 8790 while Browser access runs
- **THEN** 8787 is closed, 8790 serves the sign-in page and every session has ended

### Requirement: Home network only
While "Only devices on my home network" is on (the default), the desktop app's Browser access SHALL refuse every request whose peer address is not a loopback, private, link-local or carrier-grade NAT address with 403 and "Browser access is limited to your home network.".

#### Scenario: Address outside the home network
- **WHEN** the option is on and a request arrives from a public address
- **THEN** it is refused with 403 and no page or command is served

#### Scenario: Option off
- **WHEN** the option is off and a request arrives from a public address
- **THEN** the sign-in page is served and every other sign-in rule still applies

### Requirement: Password reset with a PIN
On the headless server, "Forgot password?" SHALL, for a home-network browser, write a one-time 8-digit PIN valid for 15 minutes to `password-reset-pin.txt` in the data folder (owner-only), log the file and its expiry without the PIN, and ask for the PIN and a new password. No new PIN SHALL be made while one is live or within a minute after one ends. Other browsers SHALL be told to reset from the home network or run `cinevault-server passwd`.

#### Scenario: Reset from the home network
- **WHEN** a browser at 192.168.1.30 asks for a reset, the person reads the PIN from `password-reset-pin.txt` and enters it with a new password
- **THEN** the password changes, every other browser is signed out, the file is gone and the browser is signed in

#### Scenario: Wrong PIN five times
- **WHEN** a wrong PIN is entered five times
- **THEN** the PIN is void, its file is deleted, and the right PIN is refused afterwards

#### Scenario: Expired PIN
- **WHEN** the right PIN is entered 16 minutes after it was created
- **THEN** it is refused, the file is deleted and the password is unchanged

#### Scenario: Asking twice
- **WHEN** a reset is asked for twice within a minute
- **THEN** the second answer points at the same file and the PIN does not change

#### Scenario: Public address
- **WHEN** a browser at a public address asks for a reset
- **THEN** it is refused with the home-network sentence and no file is written

#### Scenario: Spoofed forwarded address
- **WHEN** a request from a public peer that is not a trusted proxy carries `X-Forwarded-For: 192.168.1.30`
- **THEN** the header is ignored and the reset is refused

#### Scenario: Not in the log
- **WHEN** a PIN is created
- **THEN** the log names the file and its expiry and does not contain the PIN

### Requirement: Using a reset PIN
The server SHALL compare a reset PIN in constant time. Five wrong PINs SHALL void it and delete its file. A right PIN with a new password that is too short or does not match SHALL be refused without spending the PIN. A successful reset SHALL set the password, end every other session and stream, delete the file and sign the browser in. An expired PIN SHALL be refused and its file deleted. `cinevault-server passwd` SHALL keep working as the administrator's fallback.

#### Scenario: Reset completes
- **WHEN** the right PIN and a matching 12-character password are sent
- **THEN** the password changes, the old sessions end, the file is deleted and the browser is signed in

#### Scenario: PIN used twice
- **WHEN** the PIN that completed a reset is sent again
- **THEN** it is refused and the password is unchanged

#### Scenario: Short new password
- **WHEN** the right PIN is sent with a new password of 8 characters
- **THEN** the reset is refused for the password and the same PIN still works with a valid password

### Requirement: Password recovery in the desktop app
On the sign-in page served by the desktop app's Browser access, "Forgot password?" SHALL say "Change it in CineVault's Settings on <computer name>.", and the reset requests SHALL be refused without creating a PIN.

#### Scenario: Forgot password in the app
- **WHEN** "Forgot password?" is used on the sign-in page served by the desktop app on a computer named STUDY-PC
- **THEN** the page says "Change it in CineVault's Settings on STUDY-PC." and no PIN file is written

## MODIFIED Requirements

### Requirement: First-run welcome
On a start with no password set, the headless server SHALL serve a welcome page that asks for a new password of at least 12 characters and a confirmation, and nothing else. There SHALL be no default password and no setup token. The welcome pages SHALL be available for as long as no password exists, but only to browsers whose client address is a loopback, private (`10/8`, `172.16/12`, `192.168/16`, `fc00::/7`), link-local (`169.254/16`, `fe80::/10`) or carrier-grade NAT (`100.64/10`) address, judged by the socket peer, or by `X-Forwarded-For` only when the peer is a trusted proxy. A browser from any other address SHALL get a page that says "Finish setup from your home network." and names the server's password command, and SHALL NOT be able to set a password. The first home-network browser to open the welcome page SHALL claim the setup with a setup cookie; a second browser SHALL be refused while the claim is in use, and a claim unused for 30 minutes SHALL lapse. The welcome page SHALL say plainly that setup must be finished before the server is made reachable from the internet. The desktop app's Browser access SHALL never show the welcome page; its password is set in the app's Settings.

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

### Requirement: Server settings
Settings SHALL gain a Server section, shown only when the page is served to a browser by the server or by the desktop app's Browser access, with the address and port (read-only, with the reason in a hint), the running version, the session lifetime, "Change password", "Sign out every session", "Allow permanent delete", the Kodi connection (host, port, optional credentials, path mapping) and the trusted proxy state. When the page is served by the desktop app, the hint SHALL say that Browser access is managed in CineVault's Settings on that computer. Changing the password SHALL require the current password and SHALL end every other session. Permanent delete SHALL be off by default.

#### Scenario: Change the password
- **WHEN** the user changes the password from Settings › Server
- **THEN** the current session stays signed in, every other session is signed out, and the next sign-in needs the new password

#### Scenario: Desktop app
- **WHEN** Settings is opened in the desktop app's own window
- **THEN** no Server section is shown and the Browser access section is

#### Scenario: Permanent delete off
- **WHEN** permanent delete has not been enabled and a browser asks to erase a title
- **THEN** the request is refused and the UI offers the configured deleted-files destination instead
