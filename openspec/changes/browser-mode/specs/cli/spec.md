# Spec Delta

## MODIFIED Requirements

### Requirement: Local channel of the open app
While the CineVault app or the server is open it SHALL listen on a per-user local channel (a named pipe closed to remote clients on Windows, a user-only Unix socket on macOS and Linux, or a socket inside the container for the server) and write `automation-endpoint.json` in the data folder with the address, a random token and the protocol version. It SHALL also write `library-owner.json` with its role, process, host, protocol and heartbeat, so a client that cannot reach the channel can tell that the library is held. The app or server SHALL check the token before reading anything else and SHALL run forwarded batches on its own services, one batch at a time.

#### Scenario: Wrong token
- **WHEN** a process connects with a token that does not match
- **THEN** the app or server refuses the request without reading the batch

#### Scenario: Owner file
- **WHEN** the server holds the library
- **THEN** `library-owner.json` names the server and its role, and its heartbeat is younger than 30 seconds

### Requirement: Batches run inside the open app
The library SHALL have one writer. The program SHALL read `automation-endpoint.json` and, when an app or server answers, send the batch with the token, print the streamed progress and the report with `forwarded` set. When the endpoint says nothing is listening but `library-owner.json` reports a fresh server or app, the program SHALL exit with code 2, name the owner and SHALL NOT open the library. Only when there is no endpoint and no fresh owner SHALL the program open the library itself. When the app or server answers but refuses (wrong protocol version or token) or the connection breaks off, the program SHALL exit with code 2 and SHALL NOT open the library.

#### Scenario: App open
- **WHEN** the app is open and `cinevault movie --rename-new` is run
- **THEN** the app runs the batch, the program prints the report, and the library is opened by only one process

#### Scenario: App refuses the batch
- **WHEN** the app answers with a protocol version error
- **THEN** the program reports that the batch was not run because the app holds the library, and exits with code 2

#### Scenario: Server in a container
- **WHEN** the host program sees a fresh server owner but cannot reach the container's socket
- **THEN** it exits with code 2, names the server, and opens no library

### Requirement: Running on its own
When no app and no server are open, the program SHALL open the library from the user's data folder with the user's own keys from the credential store or, when `CINEVAULT_SECRET_KEY_FILE` is set, from the server's encrypted secrets file. Before running steps it SHALL settle renames a crash left half done and record History entries for finished batches that lack one. When the library cannot be opened, the report SHALL say to close the app or server if one is open and run the command again.

#### Scenario: Stale endpoint file
- **WHEN** the app crashed and left `automation-endpoint.json` and a stale owner file behind
- **THEN** the program finds no fresh owner, opens the library itself and runs the batch

#### Scenario: Server keys
- **WHEN** the program runs on its own with `CINEVAULT_SECRET_KEY_FILE` set
- **THEN** it reads the provider keys from the encrypted secrets file and scrapes with them

### Requirement: Earlier library needs the app first
When the data folder still holds a library or settings file of an earlier schema, the program SHALL change nothing and SHALL ask the user to start the CineVault app or the server once and then run the command again. The server SHALL perform the same fresh start the app performs.

#### Scenario: Library of an earlier schema
- **WHEN** the program finds a `library.db` of an earlier schema
- **THEN** it exits with code 2 and a message saying to start the CineVault app or server once, and no file is moved
