## ADDED Requirements

### Requirement: Big preview in the picker
A double-click on an image of the artwork picker, Space on the chosen image, or the expand button that shows on hover and focus in the corner of the preview pane SHALL open the image large, about 90% of the window over the dimmed page, fitted inside, with its type, size and source under it. It SHALL fade in and grow within 160 ms, and appear at once with reduced motion.

#### Scenario: Look closer
- **WHEN** the user double-clicks a fanart image
- **THEN** the image opens large and nothing is written

### Requirement: The big preview only looks
Any click anywhere or Esc SHALL close the big preview; nothing SHALL be chosen or written by it, and the focus SHALL return to where it was. The Left and Right arrows SHALL show the neighbouring images of the current type.

#### Scenario: Flick through
- **WHEN** the large view is open and the user presses Right
- **THEN** the next image of the type shows, and the choice in the grid is unchanged

## MODIFIED Requirements

### Requirement: Preview before an image is used
A single click on a candidate SHALL only show it in the preview beside the current image of that type; "Use this" or Enter SHALL apply it. A picture dropped anywhere on the page SHALL join the preview the same way. Nothing SHALL be written before the user applies an image.

#### Scenario: Preview first
- **WHEN** the user clicks a fanart candidate
- **THEN** the preview shows it beside "Current fanart" and nothing is written yet

#### Scenario: Replace a poster with a local file
- **WHEN** the user drops a local JPEG onto the picker and presses "Use this"
- **THEN** it is written under the poster names and the toast "Changed poster · Example Movie (2020)" offers Undo
