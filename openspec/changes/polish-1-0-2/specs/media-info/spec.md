## ADDED Requirements

### Requirement: Aspect ratio wider than the frame
The detected picture ratio SHALL be reported as measured, including ratios wider than the frame's own (a 2.39:1 picture in a 16:9 frame), and SHALL never be capped at the frame's ratio.

#### Scenario: Wide picture
- **WHEN** crop detection finds a 1920×800 picture inside a 1920×1080 frame
- **THEN** the aspect ratio is reported as about 2.40:1
