## Context

Every movie and episode keeps the name of its main video as it was first scanned (`original_filename`, also written to the NFO as `<original_filename>`). Renames never change it, so it still holds the download name after any number of renames. The template engine builds a model per title (`cv-template` `tokens::movie_model` / `tv_model`) and short aliases point into it. Presets live in `cv-rename::preset`; the profile book lists them for Settings and the first run.

Renames go through `OrganizeService::apply`, reached from the `organize_apply` command (Rename page, title and copies actions) and from the automation engine's rename step (command line and automations). Drive moves go through `ChangeSourceService::apply_with`, reached from the `apply_change_source` command. The commands live in `cv-ipc`, which the desktop app and the browser server share.

## Goals / Non-Goals

**Goals:** carry the download name's release information into new names when the user asks for it, never invent a group, and offer a switch that guarantees no rename or move happens.

**Non-goals:** a full release-name parser with languages and services as tokens of their own; changing the default presets' output.

## Decisions

### Read the release from the original file name, in `cv-template`

A small reader (`tokens::release`) works on the original file name only, as text:

- **Group.** The base name (no extension, no trailing `[...]` site tags) ends with `-GROUP`, where `GROUP` is 2 to 20 letters, digits or `_`, and the part before the dash holds at least one release tag (a resolution, source, codec, HDR or audio tag). The group is not itself a release tag (`WEB-DL` gives no group `DL`; `x264` is no group) and is not all digits. A name that starts with `[Group]` (the usual anime form) gives that group when the bracket is not a tag or a number. Otherwise the group is empty. Requiring a release tag before the dash keeps titles such as `Spider-Man.mkv` or `X-Men (2000).mkv` from giving a group.
- **Tags.** The release part starts at the first release tag (resolution, source, codec, HDR, audio, `Proper`/`Repack`, `Remux`, or a language word such as `German`, `Multi`, `DL`) that comes after the year (movies) or the episode code (episodes), and ends before the group's dash. Leading and trailing separators and brackets are trimmed; inside, the text is kept as written. A name without a tag after its year or code has no release part.
- **Quality.** Source and resolution from the name in the `Source-Resolution` form download managers use: `Bluray` (also UHD Blu-ray and BD rips), `Remux` when the name says remux, `WEBDL`, `WEBRip`, `HDTV` (or `SDTV` below 720p), `DVD` (no resolution), `CAM`, `TELESYNC`, `TELECINE`, `DVDSCR`, `REGIONAL`; the resolution as `480p`, `576p`, `720p`, `1080p` or `2160p` (`4K`/`UHD` count as `2160p`). Only one part known gives that part alone. `Proper` or `Repack` follows after a space. Stream details are not used: the token describes the release as it was named, so it never waits for the video details and never changes once they are read.

Alternatives: reusing the torrent crate's release parser would add a dependency from naming to torrents and it has no group or tag-span reader; storing the parsed values in the database would need a migration and would go stale when the reader improves. Reading at render time is cheap (a few regular expressions over one name).

### The Release info preset

- Movie folder `${title} (${year})`, movie file `${title}${ - ,edition,} (${year}) [${releaseTags}]${-,releaseGroup,}`.
- Show folder `${showTitle} (${showYear})`, season `Season ${seasonNr2}`, episode file `${showTitle} - S${seasonNr2}E${episodeNr2} - ${title} [${releaseTags}]${-,releaseGroup,}`.

A title without release information gets the Clean name (the empty `[]` is removed by the clean-up). The preset id is `release-info`. It is listed after Media-server safe.

### Metadata-only mode

The setting `renamer.metadata-only` (bool, default off) is read on every request, so turning it on takes effect at once.

- `cv-app` holds the setting (`rename_settings::metadata_only`, `METADATA_ONLY_KEY`) and the message "Metadata only is on in Settings › Naming, so files and folders are not renamed or moved." The automation rename step (command line and automations) checks it and reports the step as failed with that message.
- `cv-ipc` checks it in `organize_apply` and `apply_change_source` before any service is asked to plan, answering the error code `metadata_only`. These two commands are the only way the desktop app and the browser server rename or move; the services themselves stay unaware of settings, as they are today.
- Previews (Rename page plan, the Move menu plan) still work, so nothing breaks when the page is open as the switch changes; applying is what is refused.
- Undo of a rename or move batch stays allowed.
- UI: the sidebar hides Rename; the library, title and copies menus hide Rename and "Move to…"; the Rename page, reached by its address, shows a notice with "Open Settings" instead of the plan. Each place reads the same store value.

**Interruption and undo.** The mode adds no file operation. A rename or move that is running when the switch is turned on finishes (or stops) as it would have; the guard only refuses new ones. Nothing new needs undoing.

## Risks / Trade-offs

- [A title name ending in `-Word` after a tag-like word could read as a group] → the reader requires a release tag before the dash and rejects tag words, numbers and words with spaces or dots; the tests list the edge cases.
- [Release tags may hold characters a file name cannot] → every token value goes through the same name clean-up as all other tokens.
- [A library first scanned from already-clean names has no release information] → the tokens are empty and the preset gives Clean names, which is the honest result.
