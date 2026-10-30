# Spec Delta

## MODIFIED Requirements

### Requirement: Hand-off only
Choosing a release (double-click, Enter, "Send to torrent app" in its row or its right-click menu) SHALL hand its magnet link to the system's default torrent app, or, when the release has no magnet link, download its `.torrent` file and open it with the default app, and confirm with the toast "Sent to your torrent app". In a browser, the magnet SHALL open through the browser's own handler, and a `.torrent` SHALL download from an authenticated route so the browser can open it; the toast SHALL say "Opening your torrent app…" and SHALL always offer "Copy magnet link", with a selection fallback when the clipboard API is unavailable on a plain-HTTP page. CineVault SHALL NOT download the release's content, track it or import it. "Copy magnet link" SHALL be in the right-click menu.

#### Scenario: Send a release
- **WHEN** the user double-clicks a release in the desktop app
- **THEN** the default torrent app receives its magnet link and a toast says "Sent to your torrent app"

#### Scenario: No torrent app
- **WHEN** no app is set up for magnet links in the desktop app
- **THEN** a toast says "No torrent app is set up for magnet links" and offers "Copy magnet link"

#### Scenario: Notice not accepted
- **WHEN** a hand-off is requested before the notice was accepted
- **THEN** nothing is opened and a toast says "Accept the Torrent search notice first"

#### Scenario: Browser hand-off
- **WHEN** the user sends a release from a browser
- **THEN** the magnet opens through the browser, the toast says "Opening your torrent app…" and "Copy magnet link" is offered

#### Scenario: Copy without the clipboard API
- **WHEN** the page is served over plain HTTP on the LAN and the browser has no clipboard API
- **THEN** "Copy magnet link" selects the link so the user can copy it
