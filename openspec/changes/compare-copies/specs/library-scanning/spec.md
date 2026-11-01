## ADDED Requirements

### Requirement: A library folder reached another way
The app SHALL refuse to add a folder that is, lies inside or contains a library folder of the same library reached another way (a network share, a mapped drive, a link or a substituted drive letter), with a message naming that library folder. A library folder that cannot be reached SHALL be compared by its path text only. Remapping a library folder onto another path of the same folder SHALL be allowed.

#### Scenario: The same folder through a network path
- **WHEN** `D:\Movies` is a movie folder, this computer shares it as `\\SERVER\Movies`, and the user adds `\\SERVER\Movies`
- **THEN** the folder is not added and the message reads "\\SERVER\Movies is the library folder D:\Movies reached another way (a network share, mapped drive or link). Each folder can be in the library once."

#### Scenario: A folder inside through a link
- **WHEN** `D:\Movies` is a movie folder, `E:\Films` is a link to it, and the user adds `E:\Films\Action`
- **THEN** the folder is not added and the message reads "E:\Films\Action is already covered by the library folder D:\Movies."

#### Scenario: Moving a library folder to its network path
- **WHEN** the user changes the path of the library folder `D:\Movies` to `\\SERVER\Movies`, the share of the same folder
- **THEN** the change is allowed
