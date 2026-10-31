## MODIFIED Requirements

### Requirement: Move to with space before and after
"Move to…" in the Library selection bar, the right-click menu, the title page header ("Move") and the Drives selection bar SHALL open the same menu: "Move N titles · <size> to", then each other library folder of the same type as a row with its drive's name, the folder's label and the drive's free space on the first line, the folder's path on the second line shortened in the middle with the whole path as its tooltip, and a used-space bar with "→ <free after> free after"; and below the rows how much the source drive frees. A destination that cannot take the titles SHALL stay in the list, reachable with the arrow keys, show the reason on its row in the warning colour, and ignore choosing. Only the list SHALL scroll, vertically, when it does not fit the window. A press outside the menu SHALL close it and still reach what was pressed; pressing the control that opened it SHALL close it; Esc SHALL close it and keep the selection. Nothing SHALL touch the disk until the user chooses a destination.

#### Scenario: Space before and after
- **WHEN** the user moves 3 titles totalling 198.7 GB from D: to `E:\Archive\Movies`
- **THEN** the menu shows "5.81 TB free" and "→ 5.61 TB free after" for it and "D: goes from 1.21 TB to 1.40 TB free"

#### Scenario: Not enough room
- **WHEN** the selection is 2.1 TB and a destination has 41 GB free
- **THEN** that destination is disabled with "Not enough space: needs 2.1 TB, 41 GB free."

#### Scenario: Offline or no other folder
- **WHEN** a destination is offline, or no other folder of the type exists
- **THEN** the destination is disabled with "The drive is offline. Connect it to move here.", or the menu says "There is no other movie folder to move to." and "Add one in Settings › Library folders." with Open Settings

#### Scenario: A click outside reaches the list
- **WHEN** the menu is open and the user clicks another title's checkbox
- **THEN** the menu closes and the title is selected with that one click

#### Scenario: Measuring
- **WHEN** the free space of the destinations is still being read
- **THEN** the menu shows placeholder rows and says "Measuring free space…" to assistive technology
