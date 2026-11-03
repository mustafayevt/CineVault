## ADDED Requirements

### Requirement: Faster start over HTTPS
When the browser version is reached over HTTPS, it SHALL register a service worker that keeps the hashed build files for a faster start and shows "Can't reach CineVault" with Try again when a page can't be loaded because the server does not answer. The worker SHALL NOT store pages, commands, streams, images or media. Over plain HTTP and in the desktop app no service worker SHALL be registered. The worker script SHALL be served without a session and checked for a new version on every start.

#### Scenario: Server away
- **WHEN** an installed app reached over HTTPS opens while the server does not answer
- **THEN** it shows "Can't reach CineVault" with Try again instead of the browser's error page

#### Scenario: Plain HTTP
- **WHEN** the browser version is opened at `http://192.168.1.20:8787`
- **THEN** no service worker is registered and it works as before

#### Scenario: Nothing private stored
- **WHEN** a signed-in person browses the library, opens images and plays a file over HTTPS
- **THEN** the worker's storage holds only build files under `/assets/` and the offline page
