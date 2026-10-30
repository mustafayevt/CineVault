# cli Specification

## Purpose
Covers the `cinevault` command-line program in `cli/`: running the library's batch steps (update, scrape, media info, aspect ratio, rename, NFO) on the movie or TV library without a window, selecting titles by data source, folder or quick-search query, the dry run, plain-text and JSON reports, the exit codes, the provider credential listing, and how a batch is handed to the CineVault app when the app is open and holds the library.

## Requirements

### Requirement: Commands
The program SHALL be named `cinevault` and SHALL offer three subcommands: `movie` (batch steps on the movie library), `tvshow` (batch steps on the TV library) and `providers` (each metadata provider's importance and whether the user's key is active, missing or rejected). Run without arguments, it SHALL print its help, which includes examples and the fixed step order. It SHALL support `--version`.

#### Scenario: No arguments
- **WHEN** `cinevault` is run with no arguments
- **THEN** it prints the help with the `movie`, `tvshow` and `providers` subcommands and changes nothing

#### Scenario: Unknown option refused
- **WHEN** `cinevault movie --export` is run
- **THEN** the program rejects the unknown option with a usage error and changes nothing

### Requirement: Update, scrape and media info steps
The `movie` and `tvshow` subcommands SHALL offer: update (`-u`, `--update-all`), which rescans the selected data sources (or all of the library's kind) and waits for the media info of what the scan found; scrape (`-n`/`--scrape-new`, `--scrape-unscraped` or `--scrape-all`), where `--scrape-all` takes the best match and skips locked titles; and media info (`-m`, `--media-info`), which reads the media info of titles that have none.

#### Scenario: Nightly script
- **WHEN** `cinevault movie --update-all --scrape-new --rename-new` is run from a scheduler
- **THEN** new movies are found, scraped and renamed with the saved settings, and the program exits with code 0

### Requirement: Aspect ratio, rename and NFO steps
The subcommands SHALL also offer: aspect ratio (`-d`/`--aspect-ratio-new` or `--aspect-ratio-all`), measured with FFmpeg; rename (`-r`/`--rename-new` or `--rename-all`), which plans with the active naming profile, applies the clean changes through the undoable journal and reports conflicts without touching them; and NFO (`-w`, `--rewrite-nfo`), which rewrites NFO files with the saved NFO settings and never overwrites files CineVault does not own.

#### Scenario: Conflict left alone
- **WHEN** `cinevault movie --rename-all` finds a title whose new folder name is already taken
- **THEN** that title is reported as a conflict and its files are not touched, while the other titles are renamed

### Requirement: Step flags are checked
Giving two scopes of the same step SHALL be a usage error. A run with no step SHALL stop with a message asking for at least one step and exit with code 2. The program SHALL offer no trailer, subtitle, export or sync step.

#### Scenario: Two scopes of one step
- **WHEN** `cinevault movie --scrape-new --scrape-all` is run
- **THEN** the program reports a usage error and runs nothing

#### Scenario: No step given
- **WHEN** `cinevault movie --source 1` is run
- **THEN** the report says no step was asked for, suggests `--update-all`, `--scrape-new` or `--rename-all`, and the exit code is 2

### Requirement: Fixed step order
Whatever order the flags are written in, the steps SHALL run in this order: update, scrape, media info, aspect ratio, rename, NFO. Each step SHALL read the library as the steps before it left it.

#### Scenario: Flags in another order
- **WHEN** `cinevault movie -w -r -n -u` is run
- **THEN** the steps run as update, scrape, rename, NFO

### Requirement: Scope words
The scope `new` SHALL mean the titles this run's update found or changed, or, in a run without an update step, the titles flagged NEW; for TV a new episode SHALL count for its show. `unscraped` SHALL mean titles no provider has scraped, and `all` every selected title. The scrape step SHALL accept or queue titles for review exactly as the app does, and a run of the program on its own SHALL match titles only when a scrape flag is given.

#### Scenario: Rename new without an update
- **WHEN** `cinevault movie --rename-new` is run and three movies are flagged NEW
- **THEN** the rename step works on those three movies

### Requirement: Step outcomes
Each step SHALL report one status, `done`, `planned` (dry run), `skipped`, `unavailable` or `failed`, with a message. A step that cannot run because a tool or provider is missing (no metadata provider ready, MediaInfo or FFmpeg not installed) SHALL be `unavailable` with the reason. A step that needs a key or account the user has not set up SHALL be `skipped` with a sentence naming where to set it up.

#### Scenario: Aspect ratio without FFmpeg
- **WHEN** `cinevault movie --aspect-ratio-all` is run and FFmpeg is not installed
- **THEN** the aspect ratio step is `unavailable` with the reason, and the exit code is 1

### Requirement: A failed step stops the rest
When a step fails, the steps after it SHALL be reported as `skipped` with "Not run because the <step> step failed." and SHALL not run.

#### Scenario: Failed scrape
- **WHEN** the scrape step fails in a run of update, scrape and rename
- **THEN** the rename step is reported as skipped because the scrape step failed, and no file is renamed

### Requirement: Selection
The selectors SHALL be `--source N|FOLDER` (a data source by its 1-based position in that library or by its folder; repeatable), `--path FOLDER` (the titles inside a folder; repeatable) and `--query SEARCH` (a quick search in the search box's grammar, such as `hdr:dv year:2010..2019 -watched`). Selectors SHALL narrow each other; with none, the whole library of the kind SHALL be selected. Relative folders SHALL be made absolute by the program.

#### Scenario: Scrape one source
- **WHEN** `cinevault movie --scrape-unscraped --source 2` is run
- **THEN** only the unscraped titles of the second movie data source are scraped

### Requirement: A selection that cannot be made stops the batch
An unknown source, a path outside every data source or a query term that cannot be read SHALL stop the batch before any step, with a message and exit code 2.

#### Scenario: Unreadable query
- **WHEN** `cinevault movie --rename-all --query "year:abc"` is run
- **THEN** the program says the search was not understood, runs no step, and exits with code 2

#### Scenario: Path outside the library
- **WHEN** `--path E:\Elsewhere` is given and no data source contains that folder
- **THEN** the program says the path is not inside any data source and exits with code 2

### Requirement: Dry run
`--dry-run` SHALL plan every step and change nothing: no rescan, no job, no file and no library row. The rename step SHALL list every title with the folder and file name before and after; the other steps SHALL report what they would do.

#### Scenario: Dry-run rename as JSON
- **WHEN** `cinevault movie --rename-all --dry-run --json` is run
- **THEN** the planned before and after paths are printed as JSON with step status `planned`, and no file is changed

### Requirement: Plain-text report
Without `--json`, the program SHALL print one line per step with its status and message, then one line per title with its result, followed by the before and after names when they differ. Progress notes SHALL go to standard error.

#### Scenario: Text output of a rename
- **WHEN** `cinevault movie --rename-all` renames one title
- **THEN** standard output shows the rename step line, the title, and the old and new names joined by an arrow

### Requirement: JSON report
With `--json`, the program SHALL print one JSON document with `kind`, `dry_run`, `forwarded`, `selection` (`titles`, `narrowed`, `sources`), `steps` (each with `step`, `status`, `message`, `counts`, `items` and, when it can be undone, `undo`), `credentials` when the batch scrapes, and `error` when it could not start. Each item SHALL carry `id`, `title`, `year`, `status` and, for a rename, `from`, `to`, `from_file` and `to_file`. No progress notes SHALL be printed.

#### Scenario: Rename item in JSON
- **WHEN** a dry-run rename plans to move `D:\Movies\example.movie.2020.1080p` to `D:\Movies\Example Movie (2020)`
- **THEN** the item has `from`, `to`, `from_file` and `to_file` with those folder and file names

### Requirement: Exit codes
The program SHALL exit with `0` when every step worked, `1` when a step failed or was unavailable or any title failed, and `2` when the command could not run (bad flags, no step, a selection that cannot be made, a library that cannot be opened, or an app that refused the batch).

#### Scenario: One title fails
- **WHEN** the scrape step finishes but one title's scrape failed
- **THEN** the exit code is 1

### Requirement: Provider credentials without key material
`cinevault providers` SHALL list each metadata provider with its name, importance (`required`, `recommended` or `optional`) and state (`active`, `missing` or `rejected`), as a table or, with `--json`, as a JSON array whose entries have exactly `provider`, `name`, `importance`, `state` and `active`. The listing and every batch report SHALL be built from the credential status only, so no key can appear in the output.

#### Scenario: Keys never printed
- **WHEN** a TMDB key is saved and `cinevault providers --json` is run
- **THEN** TMDB shows `"state": "active"` and the output contains no part of the key

### Requirement: Local channel of the open app
While the CineVault app or the server is open it SHALL listen on a per-user local channel (a named pipe closed to remote clients on Windows, a user-only Unix socket on macOS and Linux, or a socket inside the container for the server) and write `automation-endpoint.json` in the data folder with the address, a random token and the protocol version. It SHALL also write `library-owner.json` with its role, process, host, protocol and heartbeat, so a client that cannot reach the channel can tell that the library is held. The app or server SHALL check the token before reading anything else and SHALL run forwarded batches on its own services, one batch at a time.

#### Scenario: Wrong token
- **WHEN** a process connects with a token that does not match
- **THEN** the app or server refuses the request without reading the batch

#### Scenario: Owner file
- **WHEN** the server holds the library
- **THEN** `library-owner.json` names the server and its role, and its heartbeat is younger than 30 seconds

### Requirement: Batches run inside the open app
The library SHALL have one writer. The program SHALL read `automation-endpoint.json` and, when an app or server answers, send the batch with the token, print the streamed progress and the report with `forwarded` set. When the endpoint says nothing is listening but `library-owner.json` reports a fresh server or app, the program SHALL exit with code 2, name the owner and SHALL NOT open the library. Only when there is no endpoint and no fresh owner SHALL the program open the library itself. When the app or server answers but refuses (wrong protocol version or token) or the connection breaks off, the program SHALL exit with code 2 and SHALL NOT open the library.

#### Scenario: App open
- **WHEN** the app is open and `cinevault movie --rename-new` is run
- **THEN** the app runs the batch, the program prints the report, and the library is opened by only one process

#### Scenario: App refuses the batch
- **WHEN** the app answers with a protocol version error
- **THEN** the program reports that the batch was not run because the app holds the library, and exits with code 2

#### Scenario: Server in a container
- **WHEN** the host program sees a fresh server owner but cannot reach the container's socket
- **THEN** it exits with code 2, names the server, and opens no library

### Requirement: Running on its own
When no app and no server are open, the program SHALL open the library from the user's data folder with the user's own keys from the credential store or, when `CINEVAULT_SECRET_KEY_FILE` is set, from the server's encrypted secrets file. Before running steps it SHALL settle renames a crash left half done and record History entries for finished batches that lack one. When the library cannot be opened, the report SHALL say to close the app or server if one is open and run the command again.

#### Scenario: Stale endpoint file
- **WHEN** the app crashed and left `automation-endpoint.json` and a stale owner file behind
- **THEN** the program finds no fresh owner, opens the library itself and runs the batch

#### Scenario: Server keys
- **WHEN** the program runs on its own with `CINEVAULT_SECRET_KEY_FILE` set
- **THEN** it reads the provider keys from the encrypted secrets file and scrapes with them

### Requirement: Earlier library needs the app first
When the data folder still holds a library or settings file of an earlier schema, the program SHALL change nothing and SHALL ask the user to start the CineVault app or the server once and then run the command again. The server SHALL perform the same fresh start the app performs.

#### Scenario: Library of an earlier schema
- **WHEN** the program finds a `library.db` of an earlier schema
- **THEN** it exits with code 2 and a message saying to start the CineVault app or server once, and no file is moved
