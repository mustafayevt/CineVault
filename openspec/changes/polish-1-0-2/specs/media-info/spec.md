## ADDED Requirements

### Requirement: Aspect ratio wider than the frame
The detected picture ratio SHALL be reported as measured, including ratios wider than the frame's own (a 2.39:1 picture in a 16:9 frame), and SHALL never be capped at the frame's ratio.

#### Scenario: Wide picture
- **WHEN** crop detection finds a 1920×800 picture inside a 1920×1080 frame
- **THEN** the aspect ratio is reported as about 2.40:1

### Requirement: Reload media info on demand
Reload media info SHALL forget the stored file details of the chosen titles (a show: of its episodes), clear the "unreadable" marks of their videos and read the files again, never from a sidecar, as a background job named "Reload media info" that shows in the sidebar progress card and can be cancelled. Titles that cannot be read again SHALL be listed in the notification centre. Without MediaInfo installed nothing SHALL be cleared.

#### Scenario: Only the chosen titles
- **WHEN** two movies hold out-of-date details and one of them is reloaded
- **THEN** only that movie's file is read again and its details match the file, and the other keeps what it had

#### Scenario: MediaInfo missing
- **WHEN** MediaInfo is not installed and the user reloads a title
- **THEN** its stored details stay as they were and the notification centre says MediaInfo is needed
