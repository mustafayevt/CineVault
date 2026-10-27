# Golden files

CineVault's golden files: fixture inputs and the exact output CineVault produces for them. They pin, byte for byte, the NFO files CineVault writes in the Kodi, Jellyfin, Emby and MediaPortal formats, the folder and file names the renamer builds, and what the name parsers read from file and folder names. Any change in that output shows up as a failing case.

```
tests/compat/
  fixtures/    inputs: file trees, entities, templates, settings (written by hand)
  expected/    CineVault's output for every input (written by the update mode, never by hand)
```

The runner is the `cv-compat` crate.

## Running

```
cargo test -p cv-compat                                     # every case, plus the runner's own tests
cargo test -p cv-compat --test suite -- --nocapture         # prints the per-case report
cargo run -p cv-compat --bin compat-report -- --all         # one line per case
```

Every case ends as pass, fail or pending (no handler registered for its family). The suite fails on any failing or pending case.

## Updating after an intended change

When a change in behaviour is intended, write CineVault's current output over the expected files:

```
CINEVAULT_UPDATE_COMPAT=1 cargo test -p cv-compat --test suite
# or
cargo run -p cv-compat --bin compat-report -- --update
```

Nothing is written unless every case produced output. Then **review the diff of `tests/compat/expected/` before committing it**: every changed line is a change users will see in their NFO files, names or scan results. Commit the code change and the golden files together.

The output is deterministic: the handlers pin the date added of entities without one (`2024-01-01 00:00:00`), write `CineVault` without a version in NFO comments, use the placeholder `cv-compat-api-key` where an NFO carries an API key, and use `/` as the path separator for movies and NFOs. Two runs produce identical files.

## Families

| Family | Inputs | Expected output |
|---|---|---|
| `classification` | `fixtures/classification.json` | `expected/classification.json` |
| `title-parsing` | `fixtures/title-parsing.json` | `expected/title-parsing.json` |
| `id-parsing` | `fixtures/id-parsing.json` | `expected/id-parsing.json` |
| `stacking` | `fixtures/stacking.json` | `expected/stacking.json` |
| `episodes` | `fixtures/episodes.json` | `expected/episodes.json` |
| `nfo` | `fixtures/nfo/cases.json`, `fixtures/nfo/entities/*.json` | `expected/nfo/<case id>/<relative path>.nfo`, `expected/nfo/index.json` |
| `rename-destinations`, `rename-trees` | `fixtures/rename/destinations.json`, `fixtures/rename/trees.json`, `fixtures/rename/entities/*.json` | `expected/rename/destinations.json`, `expected/rename/trees.json` |
| `rename-destinations-tv`, `rename-trees-tv` | `fixtures/rename/destinations-tv.json`, `fixtures/rename/trees-tv.json` | `expected/rename/destinations-tv.json`, `expected/rename/trees-tv.json` |

Renamer token renderers are covered by the `movie-token` and `tv-token` cases in `destinations*.json`. To add a family, implement `cv_compat::Handler` for it in `crates/cv-compat/src/handlers.rs`.

### File formats

All JSON files have `"version": 1` and a `"cases"` array with one case per line. Every case has a unique `id`; an expected file lists the same ids with the results.

- **classification:** `{id, path}` with `path` relative to a data source (no file needs to exist). Result: `{id, type, disc_file, main_disc_identifier}` where `type` is the media file type name.
- **title-parsing:** `{id, input, bad_words?}` (lowercase regexes). Result: `{id, title, year}`; an empty `year` means none.
- **id-parsing:** `{id, input}`. Result: `{id, imdb, tmdb, tvdb}`; a missing id is `""` (IMDb, TVDB) or `0` (TMDB).
- **stacking:** `{id, input}`, evaluated both as a file name and as a folder name. Result: `{id, marker, number, prefix, cleaned, folder_marker, folder_cleaned}`.
- **episodes:** `{id, input, show}` with a path relative to the show. Result: `{id, season, episodes, date, stacking_marker_found, cleaned_title}`; `season` is `-1` when none was found, `date` is `yyyy-MM-dd` or `""`.
- **nfo:** a case is `{id, kind, entity, profile, settings?, existing_nfo?, write?}`. `kind` is `movie`, `movie-set` or `tv`. `entity` names a file in `fixtures/nfo/entities/`. `profile` names a settings profile in `cases.json`; `settings` overrides it (`movie` and `tvShow` objects of setting names for the connector, naming and options). `existing_nfo` is `{file, content}` present before the run, to test carried-over and unknown content. `write` (TV only) selects `show`, `seasons` and `episodes`. The expected folder holds every NFO the case wrote, byte for byte (UTF-8, CRLF; `.gitattributes` keeps git from converting them).
- **rename destinations:** `{id, kind, entity, template, episode?, episodes?, renamer?}`. Kinds: `movie-folder`, `movie-file`, `movie-token` (the raw render before cleanup), `tv-show-folder`, `tv-season-folder`, `tv-episode-file`, `tv-token`, `invalid-characters`, `pattern-check`. `renamer` overrides renamer profile fields. TV episodes are selected as `{season, episode}`. Result: `{id, result}`, or the check values for `pattern-check` (`folder_unique`, `file_valid`, `tv_recommended`, `tv_pattern_error`).
- **rename trees:** `{id, kind, entity, profile, files?, folder?, renamer?, settings?}`. `files` replaces the entity's files with a tree (a path string is classified by name; an object adds media info such as `videoCodec`, `audio`, `subtitles`, or `content` for a file written to disk, since only parsable NFOs are renamed). Result: `{id, new_path, needs_rename, problems, groups}`, as the Rename preview groups them; each group is `{old, new, duped}` with sorted relative paths (`new` may point outside the folder with `../` when a file is left where it is).
- **entities:** one object per movie or show with plain field names (`title`, `year`, `plot`, `certification`, dates as `yyyy-MM-dd[ HH:mm:ss]`), plus `ids`, `ratings`, `genres`, `tags`, `actors`/`directors`/`writers`/`producers`, `trailers`, `artwork` (type to URL), `files` (with `audio` and `subtitles`), and for shows `episodes`, `seasons` and `episodeGroups`.

TV templates in `destinations-tv.json` contain no path separators, so their results are the same on every platform.

## Other hand-written fixtures in this folder

These are not run by the families above; each has its own test.

- `scan/movie-trees.json`: the file trees of the movie folder walker. Each case lists the files of a data source and the movies, with their files, that `cv_scan::walk` must produce (single-movie, multi-movie, disc, stacked and skip cases). Run with `cargo test -p cv-scan --test movie_trees`.
- `scan/tv-trees.json`: the same for the TV show walker (season folders, multi-episode, date-based, anime, specials, stacked parts, disc episodes, season artwork, subtitles in sub-folders, skip rules). Run with `cargo test -p cv-scan --test tv_trees`.
- `rename/presets.json`: the Clean, Detailed and Media-server safe naming presets rendered against fixture entities (`rename/entities/` first, then `fixtures/rename/entities/`). Run with `cargo test -p cv-compat --test presets`.
- `artwork/naming.json`: the exact artwork file names `cv_artwork::naming` writes for each media-centre preset and naming choice, including collection artwork. Run with `cargo test -p cv-artwork --test naming_fixtures`; that test also checks the artwork files the renamer plans in `trees.json` and `trees-tv.json`, so the writer and the renamer cannot drift apart.

Edit these by hand and add one case per behaviour.
