## Why

An Ultra HD Blu-ray folder with Dolby Vision showed HDR10 only. The disc carries Dolby Vision profile 7: the enhancement layer (with the Dolby Vision metadata) is a second HEVC stream in the main clip, packet id 0x1015, and the clip information file marks that stream as Dolby Vision. The stream file has no Dolby Vision descriptor, so MediaInfo reports two plain HDR10 HEVC streams, and CineVault mapped only the first. A survey of a real library (more than 300 files and three Blu-ray folders, read only) with MediaInfo 26.10 found more places where the stored details did not match the media:

- Blu-ray folders had no audio or subtitle languages: a disc keeps them in its clip information, not in the stream file;
- some E-AC-3 tracks, in Blu-ray stream files and in Matroska, are reported by MediaInfo as `AC-3` with a dependent substream, and were stored as AC3;
- DTS:X 7.1.4 tracks are reported with 12 channels (the four height speakers counted) and were shown as 11.1;
- interlaced video stored as separate fields reports the field rate (59.94) as its frame rate;
- the Dolby Vision profile of AV1 (`dav1.10`) was read as profile 1, and the libraries that only give the combined `HDR_Format/String` gave no profile at all.

Videos already in the library keep the wrong details until their media info is read again.

## What Changes

1. **Dolby Vision in a second video stream.** The main picture is never a Blu-ray enhancement layer; a Dolby Vision layer found in another video stream of the file (or the enhancement layer of an Ultra HD Blu-ray stream file) is added to the main picture's HDR formats and details.
2. **Blu-ray clip information.** The disc analysis reads the main clip's `CLIPINF/nnnnn.clpi` (or its `BACKUP` copy): the language of each audio and subtitle stream fills the language MediaInfo did not report, and a video stream the clip marks as Dolby Vision gets the Dolby Vision layer. What MediaInfo reports always wins.
3. **Audio:** E-AC-3 reported as `AC-3` with Dolby Digital Plus, a dependent substream or joint object coding is E-AC-3 (Atmos with object coding); height speakers are not counted as channels.
4. **Video:** interlaced video stored as separate fields takes the frame rate, not the field rate.
5. **Dolby Vision profile:** every codec string (`dvhe`, `dvh1`, `dvav`, `dva1`, `dav1`) gives its profile, and the combined string of older libraries gives the profile and base layer.
6. **Read again once.** A library migration marks Blu-ray discs, disc images, Blu-ray stream files, interlaced videos and videos with a DTS:X track of more than 8 channels or an AC3 track of more than 6 as not read, so the media-info job reads them again with the new mapping. Nothing else is changed, and no file in a media folder is written.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `media-info`: stream details (frame rate of field-stored video, channels without height speakers), audio codec variants (E-AC-3 in Blu-ray stream files), HDR formats and exact HDR details (Dolby Vision from a separate enhancement layer, every codec string, older libraries), and the disc main title (languages and Dolby Vision from the clip information).

## Out of scope

- Resolution buckets: a cropped 720-wide DVD frame (720x384) stays 360p, as the resolution rules say (the standard-definition buckets have no width rule).
- Video bit rate when the container does not state it (many Matroska files): it stays unknown rather than estimated.
- The averaged frame rate MediaInfo reports for variable-frame-rate video is kept as reported.
- HD-DVD and DVD structures; DVD languages already come from MediaInfo.
- Choosing the main title of a Blu-ray differently: the longest playlist and its largest clip already pick the feature on the sampled discs, including a seamless-branching one.
- NFO `<fileinfo>` is written from the stored details and never read back into them, so it cannot hide probed values; nothing changes there.

## Impact

- `crates/cv-media`: `mediainfo/mapping.rs`, `disc/bluray.rs`, `disc/mod.rs`, `mediainfo/snapshot.rs` (`streams_mut`), `external.rs` (language codes for the clip information).
- `crates/cv-db`: migration 103 (`reread_media_info`).
- Tests: mapping unit tests, a recorded Ultra HD Blu-ray field set, clip information tests over folders and images, the migration test.
