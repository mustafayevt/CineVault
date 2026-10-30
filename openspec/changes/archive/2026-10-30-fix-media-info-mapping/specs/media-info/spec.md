## MODIFIED Requirements

### Requirement: Stream details
For each video the app SHALL read and store:
- **file:** container format, duration and overall bitrate;
- **video:** codec, width and height, frame rate, bit depth, bitrate, scan type and pixel aspect ratio;
- **audio streams:** codec, channels, bitrate, bit depth, sample rate, language, title and flags (default, forced, original, commentary, dub, visually impaired, karaoke);
- **subtitle streams:** format, language, title and flags (default, forced, SDH).

The main video SHALL be the first moving picture that is neither an attached cover nor the Dolby Vision enhancement layer of a Blu-ray stream file. The frame rate of interlaced video stored as separate fields SHALL be its frame rate, not its field rate. Audio channels SHALL count the main layout without height speakers, so a 7.1.4 track is stored with 8 channels.

#### Scenario: Video row
- **WHEN** a 1080p HEVC file at 23.976 frames per second is read
- **THEN** its codec, size, frame rate and bit depth are stored and shown on the Technical card

#### Scenario: Interlaced video stored as fields
- **WHEN** a 1080i file reports 59.94 fields per second stored separately, at 29.97 frames per second
- **THEN** its frame rate is stored as 29.97 and its format as 1080i

#### Scenario: Height speakers
- **WHEN** a DTS:X track is reported with 12 channels, four of them height speakers
- **THEN** it is stored with 8 channels, shown as 7.1

### Requirement: Audio codec variants
The audio codec SHALL tell the variants apart: DTS-X, DTS-HD MA, DTS-HD HRA, DTS-ES, DTS 96/24, DTS Express, TrueHD and TrueHD Atmos, E-AC-3 and E-AC-3 Atmos, AC3 and AC3 EX, and MP1, MP2 or MP3 by layer; other formats keep MediaInfo's name. A Dolby Digital Plus track that MediaInfo names AC-3 (as it does in Blu-ray stream files and some Matroska files) SHALL be E-AC-3, or E-AC-3 Atmos with object coding.

#### Scenario: Atmos track
- **WHEN** a file holds a TrueHD track with Atmos in 7.1
- **THEN** the audio stream is stored as "TrueHD Atmos" with 8 channels, shown as 7.1

#### Scenario: E-AC-3 in a Blu-ray stream file
- **WHEN** a Blu-ray stream file holds a Dolby Digital Plus track with Atmos that MediaInfo names AC-3
- **THEN** the audio stream is stored as "E-AC-3 Atmos"

### Requirement: HDR formats
The app SHALL detect Dolby Vision, HDR10+, HDR10 and HLG and their combinations, in that order, with the colour description as a fallback; HDR10+ replaces HDR10, since it carries it. A Dolby Vision layer carried by another video stream than the main picture (the enhancement layer of an Ultra HD Blu-ray, which is profile 7) SHALL count for the main picture. The NFO SHALL carry the format names, and the filters and the library's quality label SHALL tell Dolby Vision, HDR10+, HDR10 and HLG apart.

#### Scenario: Dolby Vision with an HDR10 base
- **WHEN** a file has a Dolby Vision layer over an HDR10 base layer
- **THEN** its HDR formats are stored as "Dolby Vision, HDR10"

#### Scenario: Ultra HD Blu-ray with a Dolby Vision enhancement layer
- **WHEN** an Ultra HD Blu-ray folder's main clip holds an HDR10 base layer and a Dolby Vision enhancement layer as a second video stream
- **THEN** the title's video is the 2160p base layer, its HDR formats are stored as "Dolby Vision, HDR10" and its Dolby Vision profile as 7

### Requirement: Exact HDR details
The app SHALL store the Dolby Vision profile, with the compatibility id of profiles 8 and 10 (`8.1` for an HDR10 base layer, `8.2` for SDR, `8.4` for HLG), the base layer's format, the mastering display's colour primaries and luminance range, and MaxCLL and MaxFALL. The profile SHALL be read from every Dolby Vision codec string (HEVC, AVC and AV1) and, from MediaInfo versions that only give the combined HDR text, from that text.

#### Scenario: Profile 8.1
- **WHEN** a file has a Dolby Vision profile 8 layer with an HDR10 base layer
- **THEN** its Dolby Vision profile is stored as 8.1 with the base layer HDR10

#### Scenario: AV1 profile 10
- **WHEN** an AV1 file has a Dolby Vision profile 10 layer (`dav1.10`) with an HDR10 base layer
- **THEN** its Dolby Vision profile is stored as 10.1

### Requirement: Disc main title
For DVD, Blu-ray and HD-DVD folders and for ISO images, the app SHALL find the main title without mounting anything: the longest program chain of a DVD, the longest Blu-ray playlist (a clip played several times in one playlist counting once), or the largest stream file for HD-DVD and for a structure that cannot be read. The streams SHALL come from the main title's stream file and the runtime from the main title. For a Blu-ray, what the main clip's clip information states SHALL complete the streams where MediaInfo reports nothing: the language of each audio and subtitle stream, and a Dolby Vision video stream.

#### Scenario: Blu-ray main playlist
- **WHEN** a BDMV folder has several playlists
- **THEN** the streams and the runtime come from the longest playlist

#### Scenario: A looping playlist
- **WHEN** a Blu-ray playlist plays one short clip many times over
- **THEN** each clip counts once, and the main title's runtime is the film's, not several hours longer

#### Scenario: Track languages of a Blu-ray
- **WHEN** a Blu-ray folder's stream file states no languages and its clip information lists English and French audio and subtitles
- **THEN** the audio and subtitle streams are stored with those languages

## ADDED Requirements

### Requirement: Media info read again after a mapping fix
When an update corrects how media info is mapped, the videos whose stored details the correction changes SHALL be read again once, in the background media-info job, keeping their earlier details until the new read replaces them. Videos the correction does not affect SHALL NOT be read again.

#### Scenario: Blu-ray folders after the update
- **WHEN** the app starts for the first time after the update that reads Dolby Vision from Blu-ray clip information
- **THEN** the library's Blu-ray folders, disc images and Blu-ray stream files are read again and other Matroska files are not
