## ADDED Requirements

### Requirement: Big preview in the picker
The artwork picker's preview SHALL have an expand button, and a double-click on the preview or Space with a candidate chosen SHALL open the image large over the page. Any click or Esc SHALL close it, and the Left and Right arrows MAY step to the neighbouring candidate. Nothing SHALL be written by opening the big preview.

#### Scenario: Look closer
- **WHEN** the user selects a fanart candidate and presses Space
- **THEN** the image opens large, and Esc returns to the picker with the same candidate chosen
