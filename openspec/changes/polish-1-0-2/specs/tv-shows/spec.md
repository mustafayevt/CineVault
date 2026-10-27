## ADDED Requirements

### Requirement: Episode rows animate open
Opening and closing an episode SHALL animate its height and opacity over about 180ms and rotate the row's chevron; with reduced motion on, it SHALL open and close at once.

#### Scenario: Reduced motion
- **WHEN** the system asks for reduced motion and the user opens an episode
- **THEN** the details appear without animation
