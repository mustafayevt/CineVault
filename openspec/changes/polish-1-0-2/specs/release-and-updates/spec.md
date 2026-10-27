## MODIFIED Requirements

### Requirement: Update notice
A background check that finds a newer version SHALL show a toast "CineVault <version> is available" with "Show", which opens Settings › Updates, where the release notes of that version are shown as plain text above "Update now". Nothing SHALL be downloaded or installed until the user presses "Update now", unless installing on close is on. A failed background check SHALL interrupt no one.

#### Scenario: Update found in the background
- **WHEN** a newer signed release is published and checks are on
- **THEN** a toast says "CineVault 1.2.0 is available", and nothing is installed until the user presses "Update now"

#### Scenario: Release notes shown
- **WHEN** the offered release carries notes
- **THEN** Settings › Updates shows them under the status line before the user decides
