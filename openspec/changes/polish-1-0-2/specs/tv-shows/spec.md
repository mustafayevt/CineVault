## ADDED Requirements

### Requirement: Episode rows animate open
Opening and closing an episode SHALL animate its height and fade its details over about 180ms with the rows below moving with it, turn the row's chevron, and tint the opened card; closing SHALL reverse it. Enter and Space on the episode title SHALL toggle it. With reduced motion on, it SHALL open and close at once.

#### Scenario: Reduced motion
- **WHEN** the system asks for reduced motion and the user opens an episode
- **THEN** the details appear without animation
