## ADDED Requirements

### Requirement: Installable from a browser
The browser version SHALL be installable as an app (Add to Home Screen on iOS, Install app where a browser offers it). Its manifest SHALL name CineVault, start at `/`, open full screen, use the dark window ground as its colours and list 192 and 512 pixel icons, one maskable. The browser shell and the server's own pages SHALL link the manifest and icons and carry the iOS full-screen and theme-colour tags. The desktop app window SHALL NOT link the manifest.

#### Scenario: Add to Home Screen on iOS
- **WHEN** a person opens the sign-in page in Safari on an iPhone and chooses Share, then Add to Home Screen
- **THEN** the suggested name is "CineVault" with the CineVault icon, and opening it from the home screen shows CineVault full screen without the Safari bars

#### Scenario: Every server page
- **WHEN** the sign-in, welcome or "Finish setup from your home network" page is opened
- **THEN** its head links `/manifest.webmanifest` and `/apple-touch-icon.png`

#### Scenario: Desktop window
- **WHEN** the desktop app opens its window
- **THEN** its page links no manifest

### Requirement: Install files without a session
The manifest, the home-screen icon, the manifest icons and the favicons SHALL be served without a session, the manifest as `application/manifest+json`, with a cache of at most one day. Every other interface file outside `assets/` SHALL still need a session.

#### Scenario: Manifest without a session
- **WHEN** a browser that is not signed in asks for `/manifest.webmanifest` or `/icon-512.png`
- **THEN** the server answers with the file and `Cache-Control: public, max-age=86400`

#### Scenario: Other files stay private
- **WHEN** a browser that is not signed in asks for any other interface file outside `assets/`
- **THEN** it is sent to sign in, as before
