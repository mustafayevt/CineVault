## MODIFIED Requirements

### Requirement: Drive cards
The Drives page SHALL show a card per drive holding a library folder: the drive name with its size and kind ("HDD", "SSD", "Network" or "Removable"), a "<free> free of <total>" bar ("Low: <free> free" when low), each library folder on it with its title or show count and size and a "Scan this folder" button, the space of other files as "not in library" from 1 GB, and a 2160p, 1080p, 720p and SD breakdown. "Scan this folder" SHALL scan that library folder only (see `library-scanning` "Scan one library folder"); it SHALL be keyboard reachable without choosing the card.

#### Scenario: Two folders on one card
- **WHEN** drive E: holds `E:\Archive\Movies` with 39 titles and `E:\TV` with 18 shows
- **THEN** its card lists both folders with "39 titles" and "18 shows" and their sizes, each with "Scan this folder"

#### Scenario: Scan one folder from its card
- **WHEN** the user chooses "Scan this folder" next to `E:\TV`
- **THEN** only `E:\TV` is scanned, the toast says "Scanning TV", and the card stays chosen as it was

#### Scenario: Offline card
- **WHEN** `Z:` was last reachable two days ago
- **THEN** its card is greyed, reads "Offline · last seen 2 days ago", still lists its folders, and its titles stay in the library
