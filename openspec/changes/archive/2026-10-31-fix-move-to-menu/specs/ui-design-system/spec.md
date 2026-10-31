## MODIFIED Requirements

### Requirement: Disabled controls explain themselves
A disabled control that has a reason SHALL stay focusable and show, on hover and on keyboard focus, a one-line reason and the action that enables it. A multi-line menu row that has room for it (a destination of the Move to menu) SHALL instead show its reason on the row itself at all times, in the warning colour, and name it as the row's description for assistive technology; it SHALL NOT draw a floating line inside the menu.

#### Scenario: Continue without a tested key
- **WHEN** the user focuses the disabled Continue button on the first-run TMDB key step
- **THEN** a line says "Test your TMDB key first." with "Paste the key and press Test, or skip this step."

#### Scenario: Destination without room
- **WHEN** the Move to menu lists a drive that cannot take the selection
- **THEN** its row says "Not enough space: needs 71.4 GB, 1.02 GB free." without hovering, and the menu shows no scrollbar for it
