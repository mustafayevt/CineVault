## Context

MediaInfo is the only source of stream details. Its snapshot is mapped by a pure function (`apply_snapshot`), which also serves the XML sidecar cache, so mapping fixes apply to sidecars too. Discs are read through their structure: the longest Blu-ray playlist, its largest clip fed to MediaInfo's buffer interface, and the playlist's duration put in the snapshot.

Evidence, from MediaInfo 26.10 on a real Ultra HD Blu-ray folder (one playlist, one 42 GB clip):

- the program map has HDMV registration descriptors only, no Dolby Vision descriptor;
- MediaInfo lists `Video` 0x1011 (3840x2160, `HDR_Format: SMPTE ST 2086`) and `Video` 0x1015 (1920x1080, the same HDR10 fields);
- the 0x1015 elementary stream holds HEVC NAL units of type 62 (the Dolby Vision metadata), the 0x1011 stream none;
- `CLIPINF/00000.clpi` gives 0x1011 the dynamic range type 1 (HDR10) and 0x1015 type 2 (Dolby Vision), and the audio and subtitle languages (`rus`, `eng`), which MediaInfo does not report from the stream file.

A Dolby demonstration clip (`.m2ts` without a disc around it) shows the same 0x1011 plus 0x1015 layout with type 62 units in 0x1015.

## Decisions

1. **The enhancement layer by packet id.** In a BDAV or MPEG transport stream an HEVC stream with packet id 0x1015 is the Dolby Vision enhancement layer; the Ultra HD Blu-ray format reserves that id for it. It is never the main picture, and when the main picture is HEVC its presence adds Dolby Vision profile 7. This also covers a bare `.m2ts` without clip information.
2. **The Dolby Vision layer is written in MediaInfo's own layout.** The layer added is what MediaInfo reports for profile 7 when it does recognise it (`Dolby Vision` / `dvhe.07` / `BL+EL+RPU` / compatible with `Blu-ray`, put in front of the existing layers with ` / `), so the existing HDR mapping reads it unchanged and the result equals a Matroska remux of the same disc (`Dolby Vision, HDR10`, profile `7`, base `Blu-ray`). Level and version are unknown without parsing the metadata and are left out.
3. **A Dolby Vision layer MediaInfo found in another video stream is merged.** When the main picture has no Dolby Vision layer and another (non-cover) video stream has one, that stream's Dolby Vision part of every `HDR_Format*` field is put in front of the main picture's layers.
4. **Clip information only adds.** Languages fill only streams without one; `und` is skipped; three-letter codes become the two-letter code where one exists (as MediaInfo does), else stay three letters. Dolby Vision is added only to a stream that does not already name it. The `BACKUP/CLIPINF` copy is used when the main one is missing or unreadable. A damaged file is ignored, never fatal.
5. **Height speakers.** When `ChannelLayout` lists as many speakers as the channel count and some are top or vertical-height positions (`T…`, `Vh…`), the channel count is the rest. Atmos beds were already 7.1; DTS:X now matches.
6. **E-AC-3 named as AC-3.** MediaInfo reports some E-AC-3 tracks as `AC-3`, in Blu-ray stream files (coding type 0x84) and in Matroska (`CodecID: A_EAC3`). `AC-3` with a Dolby Digital Plus commercial name, a `Dep` substream or `JOC` is E-AC-3; `JOC` or an Atmos name makes it E-AC-3 Atmos.
7. **Separated fields.** With `ScanType_StoreMethod: SeparatedFields`, `FrameRate_Original` is the frame rate.
8. **Read again by migration.** The stored rows are only changed by a new read. Migration 103 clears `container_format` (the "not read" mark) of the videos the fixes can change, which the media-info job picks up after the next start. Selection: disc folders and markers (`BDMV`, `index.bdmv`), `.m2ts`, `.mts` and `.iso` files, stored containers `m2ts`/`mts`/`ssif`/`Blu-ray Video`, interlaced videos (`exact_video_format` ending in `i`), and videos with a `DTS-X` stream of more than 8 channels or an `AC3` stream of more than 6 (AC-3 has at most 5.1, so such a stream is a mis-named E-AC-3).

## Safety

No file in a media folder is written: the clip information is read, and no sidecar is written inside a disc structure (unchanged). The migration only empties one column of the selected rows, runs inside the migration transaction after the pre-migration backup, and the old details stay visible until the new read replaces them; a video that cannot be read again keeps its unreadable mark as before. An interrupted migration rolls back.

## Risks

- A transport stream that uses 0x1015 for a second HEVC stream that is not a Dolby Vision layer would be reported as Dolby Vision. The Blu-ray format does not allow that, and the clip information agrees on both real samples.
- The re-read costs one MediaInfo read per selected video once (a disc reads 64 MB of its main clip).
