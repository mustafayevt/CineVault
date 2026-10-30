# Spec Delta

## MODIFIED Requirements

### Requirement: One scrolling Settings page
Settings SHALL be one page at `/settings`, opened from the Settings button in the sidebar, with the sections Providers, Naming, Library folders, Deleted files, Torrent sources, Appearance and Updates in that order, followed by a Server section when the page is served by the server. An index at the page's left SHALL list them; choosing an entry SHALL scroll to its section, and the entry being read SHALL show a 2px gold indicator that follows the scroll. `/settings?section=<id>` (such as `naming`, `folders` or `server`) SHALL open the page at that section.

#### Scenario: Jump to Naming
- **WHEN** the user chooses Naming in the index
- **THEN** the page scrolls to the Naming section and Naming gets the gold indicator

#### Scenario: Link from the Rename page
- **WHEN** the user follows the Rename page's link to the naming settings
- **THEN** Settings opens at the Naming section

#### Scenario: Browser Settings
- **WHEN** Settings is opened in a browser served by CineVault
- **THEN** the index ends with Server, and `/settings?section=server` opens at it

#### Scenario: Desktop Settings
- **WHEN** Settings is opened in the desktop app
- **THEN** the index has no Server entry and `/settings?section=server` opens the page at the top

## ADDED Requirements

### Requirement: Settings in a browser
When Settings is served to a browser, the controls that only make sense on the desktop SHALL be hidden or shown as not applicable with a sentence: the video player choice and custom player command, app zoom and window size, "Match Windows"/"Match macOS", install-on-close, the update check and "Check now", the Recycle Bin/Trash destination and custom trash folder when the server's deleted-files destination is `.deletedByCineVault`, and the indexer manager and own-Jackett addresses that point at the client's loopback. The Server section SHALL show the address and port (read-only), the running version, the session lifetime, "Change password", "Sign out every session", "Allow permanent delete", the Kodi connection and the trusted-proxy state. File-operation labels SHALL follow the server's operating system, while text size, date format and theme SHALL follow the client's settings.

#### Scenario: Browser providers
- **WHEN** the user opens Settings in a browser
- **THEN** the Providers, Naming, Library folders, Deleted files, Appearance and Updates sections work, and the player choice is not shown

#### Scenario: Server labels
- **WHEN** a Windows browser is used against a Linux server and the user deletes a title
- **THEN** the destination is described with the server's Linux name and the configured server folder

#### Scenario: Client appearance
- **WHEN** the client's browser prefers a light theme
- **THEN** the interface follows the client's theme choice, not the server's
