## MODIFIED Requirements

### Requirement: Library folders section
The Library folders section SHALL list every library folder with its full path, how many titles or shows it holds and whether it is watched for changes or offline, a Movies | TV shows switch and Remove, and SHALL offer "Add folder…" and "Network share…" and accept folders dropped on the window. Removing a folder SHALL keep its titles in the library as offline and confirm with an Undo toast. Scanning these folders is defined by `library-scanning`; connecting shares is defined by `network-shares`.

#### Scenario: Remove a folder
- **WHEN** the user removes `D:\Movies`, which holds 120 titles
- **THEN** a toast says "Removed D:\Movies · 120 titles stay, offline" with Undo

#### Scenario: Add a folder
- **WHEN** the user chooses "Add folder…" and picks `E:\Movies`
- **THEN** the folder is listed and a toast says "Added 1 folder"

#### Scenario: Add a network share
- **WHEN** the user chooses "Network share…" and connects `\\nas\movies`
- **THEN** the share's folder is listed and a toast says "Added 1 folder"
