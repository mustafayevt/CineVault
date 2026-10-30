# Spec Delta

## ADDED Requirements

### Requirement: Sign-in and server pages
The sign-in page, the welcome password page and the server folder browser SHALL be pages, not dialogs. The Server settings section SHALL be inline, and "Change password" and "Sign out every session" SHALL open inline forms rather than adding a second dialog. The sign-in page SHALL use the same tokens, themes and focus ring as the rest of the app, SHALL show one password field with a show/hide control, and SHALL work at 360 pixels wide. The only dialog in the app SHALL remain the permanent-delete confirmation.

#### Scenario: Sign-in at phone width
- **WHEN** the sign-in page is opened at 360 pixels wide
- **THEN** the password field, the show/hide control and the sign-in button are fully visible and keyboard reachable

#### Scenario: Folder browser is a page
- **WHEN** the user chooses "Add folder…"
- **THEN** the folder browser opens as a page, with the app's header and focus ring, not as a floating dialog

#### Scenario: Password change inline
- **WHEN** the user chooses "Change password" in Settings › Server
- **THEN** the current and new password fields appear in the section, with no extra dialog
