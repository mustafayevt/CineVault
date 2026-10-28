# media-info Specification

## Purpose
Covers the technical details CineVault reads from video files and how it shows them: video, audio and subtitle streams through the MediaInfo library, HDR formats with their exact Dolby Vision profile and mastering data, 3D layouts, resolution buckets, the main title of discs and ISO images, read limits per drive and per file, retries, the MediaInfo XML sidecar, runtime from the media, aspect ratio detection, checksums and frame grabs with FFmpeg, the audio track a title leads with, external subtitles, and the download of the pinned external tools. These values feed the Technical card, the filters, the NFO `<fileinfo>` and the naming tokens.

## Requirements

### Requirement: Stream details
For each video the app SHALL read and store:
- **file:** container format, duration and overall bitrate;
- **video:** codec, width and height, frame rate, bit depth, bitrate, scan type and pixel aspect ratio;
- **audio streams:** codec, channels, bitrate, bit depth, sample rate, language, title and flags (default, forced, original, commentary, dub, visually impaired, karaoke);
- **subtitle streams:** format, language, title and flags (default, forced, SDH).

#### Scenario: Video row
- **WHEN** a 1080p HEVC file at 23.976 frames per second is read
- **THEN** its codec, size, frame rate and bit depth are stored and shown on the Technical card

### Requirement: Audio codec variants
The audio codec SHALL tell the variants apart: DTS-X, DTS-HD MA, DTS-HD HRA, DTS-ES, DTS 96/24, DTS Express, TrueHD and TrueHD Atmos, E-AC-3 and E-AC-3 Atmos, AC3 and AC3 EX, and MP1, MP2 or MP3 by layer; other formats keep MediaInfo's name.

#### Scenario: Atmos track
- **WHEN** a file holds a TrueHD track with Atmos in 7.1
- **THEN** the audio stream is stored as "TrueHD Atmos" with 8 channels, shown as 7.1

### Requirement: Original and commentary flags
A stream SHALL be flagged original or commentary when its container says so (the Matroska original and commentary track flags, reported as the service kind) or when its title names it ("Original", "Commentary", "Director's commentary").

#### Scenario: Original flag read from Matroska
- **WHEN** a Matroska file's second audio track has the original flag set
- **THEN** that stream is stored with the original flag

#### Scenario: Commentary by title
- **WHEN** an audio track is titled "Director's commentary" without a commentary flag
- **THEN** it is stored with the commentary flag

### Requirement: HDR formats
The app SHALL detect Dolby Vision, HDR10+, HDR10 and HLG and their combinations, in that order, with the colour description as a fallback; HDR10+ replaces HDR10, since it carries it. The NFO SHALL carry the format names, and the filters and the library's quality label SHALL tell Dolby Vision, HDR10+, HDR10 and HLG apart.

#### Scenario: Dolby Vision with an HDR10 base
- **WHEN** a file has a Dolby Vision layer over an HDR10 base layer
- **THEN** its HDR formats are stored as "Dolby Vision, HDR10"

### Requirement: Exact HDR details
The app SHALL store the Dolby Vision profile, with the compatibility id of profiles 8 and 10 (`8.1` for an HDR10 base layer, `8.2` for SDR, `8.4` for HLG), the base layer's format, the mastering display's colour primaries and luminance range, and MaxCLL and MaxFALL.

#### Scenario: Profile 8.1
- **WHEN** a file has a Dolby Vision profile 8 layer with an HDR10 base layer
- **THEN** its Dolby Vision profile is stored as 8.1 with the base layer HDR10

### Requirement: HDR on the Technical card
The Technical card's Video row SHALL name the HDR formats with the Dolby Vision profile ("Dolby Vision Profile 8.1 · HDR10"). "Show all streams and details" SHALL list an HDR section with only what the file states: the Dolby Vision profile and base layer, the mastering display ("Display P3 · 0.0001 to 1000 cd/m²"), MaxCLL and MaxFALL. An SDR stream SHALL show a dash in the HDR column and no HDR section.

#### Scenario: Video row of a Dolby Vision file
- **WHEN** a 2160p file with Dolby Vision profile 8.1 over HDR10 is open
- **THEN** the Video row reads "2160p · Dolby Vision Profile 8.1 · HDR10"

### Requirement: 3D layouts
The app SHALL detect side-by-side, top-and-bottom, half side-by-side, half top-and-bottom and MVC 3D videos, from the multi-view layout MediaInfo reports, then from layout words in the file name (`3D HSBS`, `3D.TAB`), then from the frame size. When the reported frame holds both eyes side by side or one above the other, the stored size SHALL be halved to one eye.

#### Scenario: Layout from the file name
- **WHEN** a file is named `Example Movie (2020) 3D HSBS.mkv` and MediaInfo reports no layout
- **THEN** it is stored as a half side-by-side 3D video

### Requirement: Resolution buckets and categories
A video's frame size SHALL map to a resolution label from 96p to 4320p and to a category: LD (360p and below), SD (480p, 540p, 576p), HD (720p to 1440p) and UHD (2160p, 4320p). A size SHALL reach a bucket when its height is at least the bucket height minus 1%; for 720p, 1080p, 2160p and 4320p a width of at least the bucket width minus 1% SHALL also reach it, so cropped wide films keep their bucket. Filters and naming tokens SHALL use these labels.

#### Scenario: Letterboxed UHD file
- **WHEN** a video is 3840×1608
- **THEN** its label is 2160p and its category UHD

### Requirement: Disc main title
For DVD, Blu-ray and HD-DVD folders and for ISO images, the app SHALL find the main title without mounting anything: the longest program chain of a DVD, the longest Blu-ray playlist (a clip played several times in one playlist counting once), or the largest stream file for HD-DVD and for a structure that cannot be read. The streams SHALL come from the main title's stream file and the runtime from the main title.

#### Scenario: Blu-ray main playlist
- **WHEN** a BDMV folder has several playlists
- **THEN** the streams and the runtime come from the longest playlist

#### Scenario: A looping playlist
- **WHEN** a Blu-ray playlist plays one short clip many times over
- **THEN** each clip counts once, and the main title's runtime is the film's, not several hours longer

### Requirement: Disc marker files
A disc's marker file (`BDMV/index.bdmv`, `VIDEO_TS/VIDEO_TS.IFO`) SHALL be read through its disc folder, never as a media file of its own, and no sidecar SHALL be written inside a disc structure.

#### Scenario: The index file of a Blu-ray folder
- **WHEN** a scanned Blu-ray folder holds `BDMV/index.bdmv`
- **THEN** the title's details come from the disc's main title and the index file is never reported as unreadable

### Requirement: Media info read after a scan
The media info of new and changed videos SHALL be read in the background after each scan and on start, in a job named "Reading media info of N files". A file whose size and modification time are unchanged since its last read SHALL NOT be read again. One unreadable file SHALL NOT stop the job. The result SHALL be written only to files whose size and time still match the read, so a video replaced or an edit made meanwhile is never overwritten.

#### Scenario: Corrupt file in a batch
- **WHEN** one file in a 200-file media-info job is corrupt
- **THEN** that file fails with its reason and the other 199 are stored

### Requirement: Reads per drive
Reads SHALL run in parallel up to the number of CPU cores, with at most one read at a time per drive (volume), so reads on one hard disk do not slow each other down while other drives are read at once.

#### Scenario: Two drives
- **WHEN** a job holds several large files on one drive and others on a second drive
- **THEN** each drive's files are read one at a time, and both drives are read at once

### Requirement: Read time limits
Each read SHALL time out after 120 seconds plus 3 seconds per gigabyte of its file, at most 15 minutes. A cancelled job SHALL stop waiting for its reads at once.

#### Scenario: A large file
- **WHEN** a 100 GB file is read
- **THEN** it may take up to 420 seconds before it times out

#### Scenario: Cancel while reads wait
- **WHEN** the user cancels a media-info job whose reads wait for a busy drive
- **THEN** the job ends without waiting for those reads

### Requirement: Timed-out and unreadable files
A file whose read timed out SHALL stay pending and be read again on the next scan, start or Read now, with its reason kept for the Technical card. A file that could not be read for another reason SHALL be marked unreadable at its size and modification time and SHALL NOT be tried again until the file changes or its media info is reloaded on purpose, which also clears its stored details and reads it without trusting a sidecar.

#### Scenario: Timed-out file read on the next start
- **WHEN** the read of a large file timed out and the app starts again
- **THEN** the file is read again and its details are stored

#### Scenario: Broken file left alone
- **WHEN** a file MediaInfo cannot read is unchanged and the app starts again
- **THEN** it is not read again

### Requirement: Why the video details are missing
The Technical card's Video row SHALL say once, in plain words, why the details are missing: "Reading timed out. Try again", "Disc folder: reading the main title", "The video file could not be opened. Try again", "MediaInfo could not read this video", or otherwise "Video details not read". It SHALL offer "Read now", which reads every pending file and shows "Reading…" while that read runs. The Audio row SHALL NOT repeat the reason.

#### Scenario: A timed-out read on the Technical card
- **WHEN** the read of the title's video timed out
- **THEN** the Video row says "Reading timed out. Try again" with Read now, the Audio row does not repeat it, and Read now shows "Reading…" until the read ends

### Requirement: MediaInfo not installed
When MediaInfo is not installed, a scan SHALL still complete with the new titles' details pending, the app SHALL say so once per session, and the Technical card SHALL say "MediaInfo is not installed, so the video details cannot be read yet." and offer "Download MediaInfo" with its progress. Once it is installed, everything pending SHALL be read.

#### Scenario: MediaInfo missing
- **WHEN** a scan adds titles while MediaInfo is not installed
- **THEN** the titles are in the library, their details stay pending, and the Technical card offers "Download MediaInfo"

### Requirement: MediaInfo XML sidecar
The app SHALL use an up-to-date `<basename>-mediainfo.xml` or `<filename>.mediainfo.xml` next to a video in place of reading the video, which spares slow network shares. With `media.store-mediainfo-xml` on (default off), it SHALL write a sidecar in MediaInfo's standard XML format after a read, atomically; no sidecar SHALL be written inside a disc structure.

#### Scenario: Sidecar on a network share
- **WHEN** a video on a network share has an up-to-date MediaInfo XML sidecar
- **THEN** its stream details are taken from the sidecar without opening the video

### Requirement: Runtime from the media
With `media.runtime-from-media` on (default off), a title's runtime SHALL be the length of its video, the parts of a stacked title summed and rounded to the nearest minute, unless the title or its runtime field is locked.

#### Scenario: Two-part runtime
- **WHEN** a stacked movie has parts of 1 h 40 min and 1 h 32 min and the setting is on
- **THEN** its runtime is 192 minutes

### Requirement: Aspect ratio detection
With `media.detect-aspect-ratio` on (default off), the app SHALL measure the visible picture ratio, without black bars, with FFmpeg's crop detection after the media info is read, in the mode `media.aspect-mode`:

| Mode | Label | Samples | Longest spacing |
|---|---|---|---|
| `fast` | Fast (4 short samples) | 4 × 1 s | 1800 s |
| `default` | Standard (6 samples) | 6 × 2 s | 900 s |
| `accurate` | Accurate (30 samples) | 30 × 2 s | 900 s |

#### Scenario: Standard mode
- **WHEN** detection runs with the default mode on a two-hour film
- **THEN** six samples of two seconds each are measured

### Requirement: Films that change ratio
For a film that changes ratio, `media.aspect-multi-format` SHALL pick the ratio seen most often (default), the taller picture or the wider picture. Accurate mode SHALL also store a secondary ratio held by at least 6% of the samples.

#### Scenario: Multi-format film
- **WHEN** accurate detection runs on a film in a 1.78:1 frame that shows 1.78:1 for most scenes and 1.33:1 archive footage for a fifth of its length
- **THEN** 1.78:1 is stored as the aspect ratio and 1.33:1 as the secondary ratio

### Requirement: Aspect ratio limits
ISO images SHALL be skipped by aspect ratio detection, with a notice that some ISO images were skipped. A missing FFmpeg SHALL be reported once as a notice ("Aspect ratio detection needs FFmpeg"), never silently, and the media info SHALL still be stored.

#### Scenario: ISO skipped
- **WHEN** aspect ratio detection would run for an ISO image
- **THEN** it is skipped and a notice says that ISO images were skipped

### Requirement: Checksums
The app SHALL compute, per video file, a CRC-32 (8 uppercase hex digits, read from the whole file) and a picture hash (16 hex digits, from one small grey frame FFmpeg takes from the middle of the timeline), and store them; a stored value SHALL be kept unless the computation is forced. With `media.checksums-after-scrape` set to CRC-32, Picture hash or Both (default Off), they SHALL be computed after a scrape.

#### Scenario: CRC-32 after a scrape
- **WHEN** a movie is scraped while checksums after a scrape are set to CRC-32
- **THEN** its video's CRC-32 is stored

### Requirement: CRC-32 in names and NFO files
The stored CRC-32 SHALL be available as the naming token "CRC32 checksum" and SHALL be written to the NFO.

#### Scenario: CRC-32 in a file name
- **WHEN** the naming template uses the CRC32 checksum token for a movie whose CRC-32 is stored
- **THEN** the Rename preview shows the 8 hex digits in the new name

### Requirement: Frame grabs as artwork
The app SHALL offer stills from the video in the artwork picker as the source "A still from your video": eight evenly spaced frames from 10% to 90% of the video's length, taken with FFmpeg on the CPU lane, widened to their display shape for anamorphic video, and saved as JPEG files in a cache folder, for thumbs and fanart. Stacked parts SHALL count as one timeline. Disc images SHALL be refused, and a missing FFmpeg SHALL be a typed error the UI can explain.

#### Scenario: Episode thumb from a frame
- **WHEN** the user opens the thumb picker of an episode that has no thumb
- **THEN** stills from the episode are offered after the online images and one can be saved as its thumb

### Requirement: The audio track a title leads with
The audio badge of the title pages SHALL describe one track of the main video, chosen the same way for movies and episodes: the track flagged original, else the first track in the title's original language, else the default track, else the first track. A commentary track SHALL never be chosen while another track exists. Languages SHALL be compared by meaning (`en`, `eng` and `en-US` are one language). The badge's tooltip SHALL say why that track was chosen.

#### Scenario: Dub first
- **WHEN** a film whose original language is English has a French AC-3 dub as its first, default track and English TrueHD Atmos 7.1 as its second
- **THEN** the badge reads "TrueHD Atmos 7.1 · English"

#### Scenario: Commentary skipped
- **WHEN** the only original-language track is a commentary and a default track exists
- **THEN** the default track is used

### Requirement: Subtitles listed
A title's subtitles SHALL be listed read-only: embedded subtitle streams ("Embedded") and subtitle files next to the video ("{format} · external file") together, with language, flags (forced, SDH, default) and format. The app SHALL offer no way to search, download, extract or attach subtitles.

#### Scenario: Embedded and external together
- **WHEN** a movie has 7 embedded subtitle streams and 2 external `.srt` files
- **THEN** its stream list shows 9 subtitles, two of them marked as external files

### Requirement: External subtitle names
An external subtitle or audio file's language, flags and title SHALL come from its name: ISO 639-1 and 639-2 codes, locales such as `pt-BR`, and English or native language names; `forced`, `sdh`, `default`, `commentary` and `dub` as flags. A VobSub `.sub` file's language SHALL come from its `.idx` file.

#### Scenario: External language from the file name
- **WHEN** a movie has `Example Movie (2020).deu.forced.srt` next to it
- **THEN** the list shows it as German and forced, never as an unknown language

### Requirement: External tools
The app SHALL use MediaInfo (a library it loads), FFmpeg and, for torrent search, the indexer service, each from a path the user set (setting `tools.path.<tool>`, which always wins) or from a verified download in the app's own tools folder. It SHALL never install anything system-wide or change `PATH`, and no other media tool SHALL be offered or downloaded.

#### Scenario: First-run download
- **WHEN** the user finishes the first-run setup with the recommended tools ticked
- **THEN** MediaInfo and FFmpeg are downloaded in the background into the app's tools folder

### Requirement: Verified tool downloads
Downloads SHALL be pinned in a built-in manifest with an exact https URL, size and SHA-256 per platform (`windows-x86_64`, `macos-x86_64`, `macos-aarch64` and `linux-x86_64`). Archives SHALL be zip, tar.gz, tar.bz2 or tar.xz, or the program itself. A download SHALL be hashed while written to a staging folder, discarded on a mismatch, and only then unpacked; the finished folder SHALL replace the old install in one rename. A program-tree package SHALL be refused when an entry is absolute, holds `..`, a drive letter, a link or a special file, or lies outside its root.

#### Scenario: Checksum mismatch
- **WHEN** a downloaded FFmpeg archive does not match its expected SHA-256
- **THEN** it is deleted, nothing is installed, and the error says the download did not match its expected checksum

#### Scenario: Archive entry outside the package
- **WHEN** a downloaded program-tree archive holds an entry with `..` or an absolute path
- **THEN** nothing is installed and the install fails with a plain error

#### Scenario: Linux downloads
- **WHEN** the first-run setup finishes on Linux with the recommended tools ticked
- **THEN** `libmediainfo.so.0` and `ffmpeg` are downloaded from the `linux-x86_64` entries into the app's tools folder, and the Technical card reads video details

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
