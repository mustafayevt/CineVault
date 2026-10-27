## ADDED Requirements

### Requirement: Detected ratio is the picture's own shape
Samples SHALL skip the first 2% and the last 8% of the runtime, stacked parts counting as one timeline. Each sample SHALL be rounded to the nearest standard ratio, or to the next wider one while `media.aspect-round-up` is on, and the samples vote. The detected ratio SHALL be stored as measured, wider or narrower than the frame, because black bars above and below make a picture wider than its frame.

#### Scenario: Picture narrower than its frame
- **WHEN** a 1920×1080 video shows a 4:3 picture with black bars at the sides
- **THEN** its aspect ratio is stored as 1.33:1

#### Scenario: Scope film in a 16:9 frame
- **WHEN** crop detection finds a 1920×804 picture inside a 1920×1080 frame
- **THEN** the aspect ratio is stored as 2.39:1

### Requirement: Reload media info on demand
Reload media info SHALL forget the stored file details of the chosen titles (a show: of its episodes), clear the "unreadable" marks of their videos and read the files again, never from a sidecar, as a background job named "Reload media info" that shows in the sidebar progress card and can be cancelled. Titles that cannot be read again SHALL be listed in the notification centre. Without MediaInfo installed nothing SHALL be cleared.

#### Scenario: Only the chosen titles
- **WHEN** two movies hold out-of-date details and one of them is reloaded
- **THEN** only that movie's file is read again and its details match the file, and the other keeps what it had

#### Scenario: MediaInfo missing
- **WHEN** MediaInfo is not installed and the user reloads a title
- **THEN** its stored details stay as they were and the notification centre says MediaInfo is needed

## REMOVED Requirements

### Requirement: Aspect ratio sampling and rounding
**Reason**: It refused any detected ratio wider than the frame, which threw away every letterboxed (scope) film's real ratio and left the frame's ratio on the Technical card.
**Migration**: Replaced by "Detected ratio is the picture's own shape"; samples still skip the first 2% and the last 8% of the runtime, with stacked parts counting as one timeline.
