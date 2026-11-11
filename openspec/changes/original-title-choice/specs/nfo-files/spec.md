## ADDED Requirements

### Requirement: NFO title follows the choice of title
The NFO `<title>` SHALL be the title in use and `<originaltitle>` the original title. While a movie or show uses its original title, CineVault's own elements SHALL hold `<localized_title>` with the kept localized title, after `original_language`; it SHALL be left out otherwise, so the NFO of a title that keeps its localized title is unchanged.

#### Scenario: Original title in the NFO
- **WHEN** a movie with the localized title "The Sleeping City" and the original title "La Ciudad Dormida" uses its original title and its NFO is written
- **THEN** the NFO holds `<title>La Ciudad Dormida</title>`, `<originaltitle>La Ciudad Dormida</originaltitle>` and `<localized_title>The Sleeping City</localized_title>`

### Requirement: Reading the choice of title back
Reading an NFO back SHALL keep or restore the choice of title from `<title>`, `<originaltitle>` and `<localized_title>`. When the title read is not the original title, it SHALL become the title in use and the choice SHALL return to Localized.

#### Scenario: A library built again from the files
- **WHEN** that NFO is read into a new library
- **THEN** the movie uses its original title and "The Sleeping City" is its localized title, so it can switch back

#### Scenario: A title edited elsewhere
- **WHEN** that NFO's `<title>` is changed to "Sleeping City" outside CineVault and the NFO is read back
- **THEN** the movie's title is "Sleeping City" and it uses its localized title
