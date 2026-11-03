## MODIFIED Requirements

### Requirement: Drives in the sidebar
The sidebar SHALL list each drive that holds a library folder with its name, a thin used-space bar and "<free> free". A low drive SHALL show its bar and note in the warning colour; an offline drive SHALL be greyed with "Offline". The tooltip SHALL read "<name> · <free> free of <total>". Clicking a drive SHALL open the Drives page with that drive chosen, also when the Drives page is already open, and while the Drives page is open the sidebar SHALL show the chosen drive as selected. The Drives item SHALL show a warning pill "N low" while any drive is low.

#### Scenario: Low drive
- **WHEN** drive C: has 41 GB free of 931 GB
- **THEN** the sidebar shows C: with a warning-coloured bar and "41 GB free", and the Drives item shows "1 low"

#### Scenario: Offline drive
- **WHEN** the network drive holding `Z:\Shows` does not answer
- **THEN** the sidebar shows that drive greyed with "Offline"

#### Scenario: Clicking a sidebar drive chooses it
- **WHEN** the Library is open and the person clicks "D: Media" in the sidebar
- **THEN** the Drives page opens with the D: card chosen and the titles of D: on the right, and the sidebar shows D: as selected

#### Scenario: Sidebar and cards agree
- **WHEN** the Drives page shows D: and the person clicks "E: Archive" in the sidebar, then chooses the D: card
- **THEN** the page first shows E: with E: selected in the sidebar, then D: with D: selected in the sidebar, and Back returns to E:

### Requirement: Drives page header
The Drives page header SHALL say how many drives there are, how many are offline, and when the numbers were last read, with a Refresh button that reads every drive again at once. While it reads, the button SHALL show that it is working and SHALL NOT start a second read. With no library folder the page SHALL say "No drives to show yet" with "Open Settings".

#### Scenario: One drive offline
- **WHEN** four drives hold library folders and one does not answer
- **THEN** the header reads "4 drives · 1 offline" with "Updated just now"

#### Scenario: Refresh after copying files
- **WHEN** the header says "Updated 2 minutes ago", 5 GB were copied onto D: outside CineVault, and the person clicks Refresh
- **THEN** the free space of D: shows 5 GB less on the card and in the sidebar, and the header says "Updated just now"

### Requirement: Space numbers stay current
Space numbers SHALL be read again when the Drives page opens, when the window comes back to the front, after a scan, move or delete changes files (announced at most once in ten seconds), when a drive comes or goes, every minute while the app is in front, and when the person clicks Refresh. Automatic reads SHALL NOT wake network drives more often than that.

#### Scenario: After a move
- **WHEN** a move of 198.7 GB from D: to E: finishes
- **THEN** the sidebar and Drives page show D: with 198.7 GB more free and E: with 198.7 GB less, without a restart
