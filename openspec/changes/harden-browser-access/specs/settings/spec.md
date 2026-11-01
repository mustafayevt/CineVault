## MODIFIED Requirements

### Requirement: Settings in a browser
When Settings is served to a browser, the controls that only make sense on the desktop SHALL be hidden or shown as not applicable with a sentence: the video player choice and custom player command, app zoom and window size, "Match Windows"/"Match macOS", install-on-close, the update check and "Check now", the Recycle Bin/Trash destination and custom trash folder when the server's deleted-files destination is `.deletedByCineVault`, and the indexer manager and own-Jackett addresses that point at the client's loopback. The update channel, the update signing key and each tool's path SHALL be shown read-only with "This is changed on the computer that runs CineVault.", and the tools' download and repair SHALL stay available. The Server section SHALL show the address and port (read-only), the running version, the session lifetime, "Change password", "Sign out every session", "Allow permanent delete", the Kodi connection and the trusted-proxy state. File-operation labels SHALL follow the server's operating system, while text size, date format and theme SHALL follow the client's settings.

#### Scenario: Browser providers
- **WHEN** the user opens Settings in a browser
- **THEN** the Providers, Naming, Library folders, Deleted files, Appearance and Updates sections work, and the player choice is not shown

#### Scenario: Server labels
- **WHEN** a Windows browser is used against a Linux server and the user deletes a title
- **THEN** the destination is described with the server's Linux name and the configured server folder

#### Scenario: Client appearance
- **WHEN** the client's browser prefers a light theme
- **THEN** the interface follows the client's theme choice, not the server's

#### Scenario: Tool paths in a browser
- **WHEN** the user opens the tools in Settings in a browser
- **THEN** each tool shows its path without a way to change it, with the sentence, and "Download" or "Repair" still works
