-- CineVault library schema, version 100.
-- This file creates the whole schema of a new library in one transaction; `user_version` is then
-- set to 100. Versions 1 to 99 belonged to earlier builds: such a file is moved aside as
-- `library-old.db` and never upgraded. Later schema changes are migrations numbered from 101.
-- Filter and sort columns are real, indexed columns. Rarely queried nested data sits in the
-- JSON column `ext`. Timestamps are epoch milliseconds; calendar dates are ISO `YYYY-MM-DD`
-- text (partial `YYYY-MM` / `YYYY` allowed), which sorts correctly.
-- Nothing here deletes library rows by itself: `item.parent_id` is RESTRICT, so removing a
-- show or set is an explicit, ordered action of the repository layer.

-- ---------------------------------------------------------------------------------------------
-- item: movies, sets, shows, seasons and episodes
-- ---------------------------------------------------------------------------------------------
CREATE TABLE item (
    id                    INTEGER PRIMARY KEY,
    kind                  TEXT    NOT NULL CHECK (kind IN ('movie', 'set', 'show', 'season', 'episode')),
    -- the show for a season/episode, the set for a movie in a set
    parent_id             INTEGER REFERENCES item (id) ON DELETE RESTRICT,

    -- shared metadata
    title                 TEXT    NOT NULL DEFAULT '',
    original_title        TEXT    NOT NULL DEFAULT '',
    english_title         TEXT    NOT NULL DEFAULT '',
    sort_title            TEXT    NOT NULL DEFAULT '',
    year                  INTEGER NOT NULL DEFAULT 0,
    plot                  TEXT    NOT NULL DEFAULT '',
    outline               TEXT    NOT NULL DEFAULT '',
    tagline               TEXT    NOT NULL DEFAULT '',
    note                  TEXT    NOT NULL DEFAULT '',
    runtime_minutes       INTEGER NOT NULL DEFAULT 0,
    certification         TEXT    NOT NULL DEFAULT '',
    release_date          TEXT,           -- movies: premiere; shows and episodes: first aired
    top250                INTEGER NOT NULL DEFAULT 0,
    production_companies  TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(production_companies)),

    -- location
    data_source           TEXT    NOT NULL DEFAULT '',
    path                  TEXT    NOT NULL DEFAULT '',
    original_filename     TEXT    NOT NULL DEFAULT '',
    date_added_ms         INTEGER NOT NULL DEFAULT 0,

    -- editing and scraping state
    locked                INTEGER NOT NULL DEFAULT 0 CHECK (locked IN (0, 1)),
    last_scraper_id       TEXT    NOT NULL DEFAULT '',
    last_scrape_language  TEXT    NOT NULL DEFAULT '',
    is_new                INTEGER NOT NULL DEFAULT 0 CHECK (is_new IN (0, 1)),
    review_state          TEXT    NOT NULL DEFAULT 'none'
                              CHECK (review_state IN ('none', 'needs_review', 'resolved')),
    -- computed from the file system; never causes a delete
    availability          TEXT    NOT NULL DEFAULT 'online'
                              CHECK (availability IN ('online', 'offline', 'missing', 'invalid')),
    availability_reason   TEXT    NOT NULL DEFAULT '',
    rename_history        TEXT    CHECK (rename_history IS NULL OR json_valid(rename_history)),
    -- the History entry of the Delete that sent this title's files to the Recycle Bin / Trash;
    -- such a row is hidden from every library list, count and total until Undo restores it
    --. NULL for every title in the library.
    trashed_entry         INTEGER,

    -- movie and episode
    watched               INTEGER NOT NULL DEFAULT 0 CHECK (watched IN (0, 1)),
    play_count            INTEGER NOT NULL DEFAULT 0,
    last_watched_ms       INTEGER,
    media_source          TEXT    NOT NULL DEFAULT '',
    edition               TEXT    NOT NULL DEFAULT '',
    is_disc               INTEGER NOT NULL DEFAULT 0 CHECK (is_disc IN (0, 1)),
    stacked               INTEGER NOT NULL DEFAULT 0 CHECK (stacked IN (0, 1)),

    -- movie only
    video_in_3d           INTEGER NOT NULL DEFAULT 0 CHECK (video_in_3d IN (0, 1)),
    multi_movie_dir       INTEGER NOT NULL DEFAULT 0 CHECK (multi_movie_dir IN (0, 1)),

    -- show only
    year_end              INTEGER NOT NULL DEFAULT 0,
    aired_status          TEXT    NOT NULL DEFAULT 'unknown'
                              CHECK (aired_status IN ('unknown', 'continuing', 'ended', 'canceled')),
    country               TEXT    NOT NULL DEFAULT '',
    -- the ordering used to display episode numbers, e.g. 'aired' or 'dvd:Production'
    episode_group         TEXT    NOT NULL DEFAULT 'aired',

    -- season only (0 = specials) and episode only
    season_number         INTEGER,
    multi_episode         INTEGER NOT NULL DEFAULT 0 CHECK (multi_episode IN (0, 1)),

    -- spoken languages, countries, show links, missing movies/episodes, season texts, offered
    -- episode groups and other rarely queried data
    ext                   TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(ext)),

    created_ms            INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    updated_ms            INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),

    -- the ISO 639-1 code of the production's own language, empty when unknown (movies and shows;
    -- seasons and episodes keep it empty and read their show's)
    original_language     TEXT    NOT NULL DEFAULT ''
);

CREATE INDEX item_parent        ON item (parent_id) WHERE parent_id IS NOT NULL;
CREATE INDEX item_kind_title    ON item (kind, title COLLATE NOCASE, id);
CREATE INDEX item_kind_sort     ON item (kind, sort_title COLLATE NOCASE, id);
CREATE INDEX item_kind_year     ON item (kind, year, id);
CREATE INDEX item_kind_added    ON item (kind, date_added_ms, id);
CREATE INDEX item_kind_release  ON item (kind, release_date, id);
CREATE INDEX item_kind_runtime  ON item (kind, runtime_minutes, id);
CREATE INDEX item_kind_source   ON item (kind, data_source, id);
CREATE INDEX item_kind_avail    ON item (kind, availability, id);
CREATE INDEX item_kind_watched  ON item (kind, watched, id);
CREATE INDEX item_kind_cert     ON item (kind, certification, id);
CREATE INDEX item_kind_medsrc   ON item (kind, media_source, id);
CREATE INDEX item_kind_edition  ON item (kind, edition, id);
CREATE INDEX item_kind_top250   ON item (kind, top250, id) WHERE top250 > 0;
CREATE INDEX item_new           ON item (kind, id) WHERE is_new = 1;
CREATE INDEX item_review        ON item (kind, id) WHERE review_state = 'needs_review';
CREATE INDEX item_path          ON item (path);
CREATE INDEX item_trashed       ON item (trashed_entry) WHERE trashed_entry IS NOT NULL;

-- ---------------------------------------------------------------------------------------------
-- media_file and stream
-- ---------------------------------------------------------------------------------------------
CREATE TABLE media_file (
    id                    INTEGER PRIMARY KEY,
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    position              INTEGER NOT NULL DEFAULT 0,
    kind                  TEXT    NOT NULL DEFAULT 'unknown',
    directory             TEXT    NOT NULL DEFAULT '',
    filename              TEXT    NOT NULL DEFAULT '',
    size_bytes            INTEGER NOT NULL DEFAULT 0,
    modified_ms           INTEGER NOT NULL DEFAULT 0,

    video_codec           TEXT    NOT NULL DEFAULT '',
    codec_id              TEXT    NOT NULL DEFAULT '',
    container_format      TEXT    NOT NULL DEFAULT '',
    exact_video_format    TEXT    NOT NULL DEFAULT '',
    video_3d              TEXT    NOT NULL DEFAULT '',
    width                 INTEGER NOT NULL DEFAULT 0,
    height                INTEGER NOT NULL DEFAULT 0,
    aspect_ratio          REAL    NOT NULL DEFAULT 0,
    aspect_ratio2         REAL    NOT NULL DEFAULT 0,
    pixel_aspect_ratio    REAL    NOT NULL DEFAULT 0,
    video_bitrate_kbps    INTEGER NOT NULL DEFAULT 0,
    overall_bitrate_kbps  INTEGER NOT NULL DEFAULT 0,
    bit_depth             INTEGER NOT NULL DEFAULT 0,
    frame_rate            REAL    NOT NULL DEFAULT 0,
    duration_secs         INTEGER NOT NULL DEFAULT 0,
    stacking              INTEGER NOT NULL DEFAULT 0,
    stacking_marker       TEXT    NOT NULL DEFAULT '',
    title                 TEXT    NOT NULL DEFAULT '',
    is_iso                INTEGER NOT NULL DEFAULT 0 CHECK (is_iso IN (0, 1)),
    is_animated_graphic   INTEGER NOT NULL DEFAULT 0 CHECK (is_animated_graphic IN (0, 1)),
    hdr_formats           TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(hdr_formats)),
    checksums             TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(checksums))
);

CREATE INDEX media_file_item     ON media_file (item_id, position);
CREATE INDEX media_file_kind     ON media_file (kind, item_id);
CREATE INDEX media_file_name     ON media_file (filename COLLATE NOCASE);
CREATE INDEX media_file_dir      ON media_file (directory);
CREATE INDEX media_file_res      ON media_file (height, width) WHERE kind = 'video';
CREATE INDEX media_file_vcodec   ON media_file (video_codec) WHERE kind = 'video';

CREATE TABLE stream (
    id                    INTEGER PRIMARY KEY,
    media_file_id         INTEGER NOT NULL REFERENCES media_file (id) ON DELETE CASCADE,
    kind                  TEXT    NOT NULL CHECK (kind IN ('audio', 'subtitle')),
    position              INTEGER NOT NULL DEFAULT 0,
    codec                 TEXT    NOT NULL DEFAULT '',
    codec_id              TEXT    NOT NULL DEFAULT '',
    language              TEXT    NOT NULL DEFAULT '',
    title                 TEXT    NOT NULL DEFAULT '',
    -- JSON array of flag names (default, forced, hearing_impaired, ...)
    flags                 TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(flags)),
    channels              INTEGER NOT NULL DEFAULT 0,
    bitrate_kbps          INTEGER NOT NULL DEFAULT 0,
    bit_depth             INTEGER NOT NULL DEFAULT 0,
    -- audio sample rate in Hz; 0 when unknown and for subtitle streams
    sample_rate           INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX stream_file     ON stream (media_file_id, kind, position);
CREATE INDEX stream_lang     ON stream (kind, language);
CREATE INDEX stream_codec    ON stream (kind, codec);

-- ---------------------------------------------------------------------------------------------
-- person and credit
-- ---------------------------------------------------------------------------------------------
CREATE TABLE person (
    id                    INTEGER PRIMARY KEY,
    name                  TEXT    NOT NULL,
    thumb_url             TEXT    NOT NULL DEFAULT '',
    profile_url           TEXT    NOT NULL DEFAULT '',
    -- provider ID map (same shape as the item IDs)
    ids                   TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(ids))
);

CREATE INDEX person_name ON person (name COLLATE NOCASE);

CREATE TABLE credit (
    id                    INTEGER PRIMARY KEY,
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    person_id             INTEGER NOT NULL REFERENCES person (id) ON DELETE RESTRICT,
    kind                  TEXT    NOT NULL DEFAULT 'other',
    role                  TEXT    NOT NULL DEFAULT '',
    position              INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX credit_item   ON credit (item_id, kind, position);
CREATE INDEX credit_person ON credit (person_id, kind);

-- ---------------------------------------------------------------------------------------------
-- rating, ext_id, tag, genre
-- ---------------------------------------------------------------------------------------------
CREATE TABLE rating (
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    source                TEXT    NOT NULL,
    value                 REAL    NOT NULL,
    votes                 INTEGER NOT NULL DEFAULT 0,
    max_value             INTEGER NOT NULL DEFAULT 10,
    -- when the rating was fetched; 0 when not known
    fetched_at_ms         INTEGER NOT NULL DEFAULT 0,
    -- the title's page on the rating source's site, empty when not known
    url                   TEXT    NOT NULL DEFAULT '',
    PRIMARY KEY (item_id, source)
) WITHOUT ROWID;

CREATE INDEX rating_source ON rating (source, value);

-- provider IDs as an open map: any provider key is accepted
CREATE TABLE ext_id (
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    provider              TEXT    NOT NULL,
    value                 TEXT    NOT NULL CHECK (value <> ''),
    PRIMARY KEY (item_id, provider)
) WITHOUT ROWID;

CREATE INDEX ext_id_lookup ON ext_id (provider, value);

CREATE TABLE tag (
    id                    INTEGER PRIMARY KEY,
    name                  TEXT    NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE item_tag (
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    tag_id                INTEGER NOT NULL REFERENCES tag (id) ON DELETE CASCADE,
    position              INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (item_id, tag_id)
) WITHOUT ROWID;

CREATE INDEX item_tag_tag ON item_tag (tag_id, item_id);

-- genres are open-ended: built-in values and free text share the table
CREATE TABLE genre (
    id                    INTEGER PRIMARY KEY,
    name                  TEXT    NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE item_genre (
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    genre_id              INTEGER NOT NULL REFERENCES genre (id) ON DELETE CASCADE,
    position              INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (item_id, genre_id)
) WITHOUT ROWID;

CREATE INDEX item_genre_genre ON item_genre (genre_id, item_id);

-- ---------------------------------------------------------------------------------------------
-- artwork_ref, trailer, episode_number
-- ---------------------------------------------------------------------------------------------
-- Remote artwork URLs by artwork type (poster, fanart, season_poster, extrafanart, ...).
-- Types that hold one image use position 0; extra fanart/thumbs use one row per image.
CREATE TABLE artwork_ref (
    id                    INTEGER PRIMARY KEY,
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    kind                  TEXT    NOT NULL,
    position              INTEGER NOT NULL DEFAULT 0,
    url                   TEXT    NOT NULL,
    -- content hash of the cached download, once fetched
    cache_hash            TEXT,
    width                 INTEGER NOT NULL DEFAULT 0,
    height                INTEGER NOT NULL DEFAULT 0,
    UNIQUE (item_id, kind, position)
);

CREATE INDEX artwork_ref_hash ON artwork_ref (cache_hash) WHERE cache_hash IS NOT NULL;

CREATE TABLE trailer (
    id                    INTEGER PRIMARY KEY,
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    position              INTEGER NOT NULL DEFAULT 0,
    external_id           TEXT    NOT NULL DEFAULT '',
    name                  TEXT    NOT NULL DEFAULT '',
    url                   TEXT    NOT NULL DEFAULT '',
    quality               TEXT    NOT NULL DEFAULT '',
    language              TEXT    NOT NULL DEFAULT '',
    provider              TEXT    NOT NULL DEFAULT '',
    in_nfo                INTEGER NOT NULL DEFAULT 0 CHECK (in_nfo IN (0, 1)),
    date                  TEXT,
    scraped_by            TEXT    NOT NULL DEFAULT ''
);

CREATE INDEX trailer_item ON trailer (item_id, position);

-- One row per numbering of an episode (aired, DVD, absolute, alternate, display).
-- `group_name` is '' for the orderings without a named group.
CREATE TABLE episode_number (
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    ordering              TEXT    NOT NULL
                              CHECK (ordering IN ('aired', 'dvd', 'absolute', 'alternate', 'display')),
    group_name            TEXT    NOT NULL DEFAULT '',
    season                INTEGER NOT NULL,
    episode               INTEGER NOT NULL,
    PRIMARY KEY (item_id, ordering, group_name)
) WITHOUT ROWID;

CREATE INDEX episode_number_lookup ON episode_number (ordering, group_name, season, episode);

-- ---------------------------------------------------------------------------------------------
-- rename_journal, job, review_queue, field_lock
-- ---------------------------------------------------------------------------------------------
-- Write-ahead journal of rename/copy/nfo/trash operations. A row is inserted as 'planned'
-- before the operation runs and moved to 'done' (or 'failed') after it. Undo replays the 'done'
-- rows of a batch in reverse.
CREATE TABLE rename_journal (
    id                    INTEGER PRIMARY KEY,
    batch_id              TEXT    NOT NULL,
    seq                   INTEGER NOT NULL,
    item_id               INTEGER REFERENCES item (id) ON DELETE SET NULL,
    op                    TEXT    NOT NULL CHECK (op IN ('move', 'copy', 'write_nfo', 'trash')),
    from_path             TEXT    NOT NULL DEFAULT '',
    to_path               TEXT    NOT NULL DEFAULT '',
    state                 TEXT    NOT NULL DEFAULT 'planned'
                              CHECK (state IN ('planned', 'done', 'failed', 'undone')),
    -- size and mtime of the destination right after the operation, checked again on undo
    size_bytes            INTEGER,
    modified_ms           INTEGER,
    error                 TEXT    NOT NULL DEFAULT '',
    created_ms            INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    finished_ms           INTEGER,
    -- how many folders above the destination the operation created (1 = the parent); undo
    -- removes only those, once empty
    created_parents       INTEGER,
    -- for a `trash` row of a move whose original went to the Recycle Bin / Trash instead of the
    -- trash folder: the bin's id of it, which Undo restores from
    bin_id                TEXT,
    UNIQUE (batch_id, seq)
);

CREATE INDEX rename_journal_item  ON rename_journal (item_id, id);
CREATE INDEX rename_journal_state ON rename_journal (state) WHERE state IN ('planned', 'failed');

CREATE TABLE job (
    id                    INTEGER PRIMARY KEY,
    kind                  TEXT    NOT NULL,
    lane                  TEXT    NOT NULL DEFAULT 'disk',
    state                 TEXT    NOT NULL DEFAULT 'queued'
                              CHECK (state IN ('queued', 'running', 'paused', 'succeeded', 'failed', 'canceled')),
    parent_job_id         INTEGER REFERENCES job (id) ON DELETE CASCADE,
    item_id               INTEGER REFERENCES item (id) ON DELETE SET NULL,
    priority              INTEGER NOT NULL DEFAULT 0,
    attempts              INTEGER NOT NULL DEFAULT 0,
    progress_done         INTEGER NOT NULL DEFAULT 0,
    progress_total        INTEGER NOT NULL DEFAULT 0,
    payload               TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(payload)),
    result                TEXT    CHECK (result IS NULL OR json_valid(result)),
    error                 TEXT    NOT NULL DEFAULT '',
    created_ms            INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    started_ms            INTEGER,
    finished_ms           INTEGER,
    -- the display name shown in Activity
    name                  TEXT    NOT NULL DEFAULT ''
);

CREATE INDEX job_state   ON job (state, lane, priority DESC, id);
CREATE INDEX job_parent  ON job (parent_job_id) WHERE parent_job_id IS NOT NULL;
CREATE INDEX job_by_item ON job (item_id) WHERE item_id IS NOT NULL;

-- Matches that need a human decision. Matching never blocks on the UI: it writes a row here.
CREATE TABLE review_queue (
    id                    INTEGER PRIMARY KEY,
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    reason                TEXT    NOT NULL DEFAULT '',
    state                 TEXT    NOT NULL DEFAULT 'pending'
                              CHECK (state IN ('pending', 'accepted', 'skipped', 'dismissed')),
    best_score            REAL,
    -- candidate list and the explainable score breakdown
    candidates            TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(candidates)),
    score_breakdown       TEXT    CHECK (score_breakdown IS NULL OR json_valid(score_breakdown)),
    created_ms            INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    resolved_ms           INTEGER
);

CREATE INDEX review_queue_state ON review_queue (state, best_score, id);
CREATE UNIQUE INDEX review_queue_open_item ON review_queue (item_id) WHERE state = 'pending';

-- Fields the user locked against scraping and merging.
CREATE TABLE field_lock (
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    field                 TEXT    NOT NULL,
    PRIMARY KEY (item_id, field)
) WITHOUT ROWID;

-- ---------------------------------------------------------------------------------------------
-- item_fts: full-text index over title, original title, sort title, plot, people, tags and
-- file names. rowid = item.id. The repositories keep it in step with the item.
-- ---------------------------------------------------------------------------------------------
CREATE VIRTUAL TABLE item_fts USING fts5 (
    title,
    original_title,
    sort_title,
    plot,
    people,
    tags,
    filenames,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);

-- removing an item removes its index row, so the index never refers to a missing item
CREATE TRIGGER item_fts_delete AFTER DELETE ON item
BEGIN
    DELETE FROM item_fts WHERE rowid = OLD.id;
END;

-- ---------------------------------------------------------------------------------------------
-- job_item: the items of a persistent job and how each one ended, so a job resumes from the
-- remaining items after a restart and a retry picks up only the failed ones.
-- ---------------------------------------------------------------------------------------------
CREATE TABLE job_item (
    job_id                INTEGER NOT NULL REFERENCES job (id) ON DELETE CASCADE,
    -- position of the item in the job's original item list
    idx                   INTEGER NOT NULL,
    -- display label, kept so history reads without decoding the payload
    label                 TEXT    NOT NULL DEFAULT '',
    -- the item as the job's worker needs it back after a restart
    payload               TEXT    NOT NULL DEFAULT 'null' CHECK (json_valid(payload)),
    state                 TEXT    NOT NULL DEFAULT 'pending'
                              CHECK (state IN ('pending', 'done', 'failed', 'rolled_back')),
    -- groupable failure reason when state = 'failed'
    reason                TEXT    NOT NULL DEFAULT '',
    finished_ms           INTEGER,
    PRIMARY KEY (job_id, idx)
) WITHOUT ROWID;

-- The items a resumed job still has to run, and the failures a retry picks up.
CREATE INDEX job_item_pending ON job_item (job_id, idx) WHERE state IN ('pending', 'rolled_back');
CREATE INDEX job_item_failed  ON job_item (job_id, idx) WHERE state = 'failed';

-- ---------------------------------------------------------------------------------------------
-- Data sources: the movie and TV folders the library is built from.
--
-- A source is never referenced by a foreign key. Items carry the root they were found under in
-- `item.data_source`, so removing a source cannot delete or orphan a library row: its items stay,
-- and show as offline until the folder is added again or the items are removed by the user.
-- Remapping a source rewrites the stored paths in place (`repo::data_sources::remap_source`).

CREATE TABLE data_source (
    id          INTEGER PRIMARY KEY,
    kind        TEXT    NOT NULL CHECK (kind IN ('movie', 'tv')),
    -- absolute root path, normalised by cv-scan's guard before it is stored
    path        TEXT    NOT NULL CHECK (path <> ''),
    -- display and scan order inside its kind, 0-based and gapless
    position    INTEGER NOT NULL DEFAULT 0,
    created_ms  INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    -- the name the user sees, such as "NAS 4K"; unique within a kind (checked by the repository)
    label       TEXT    NOT NULL DEFAULT '',
    -- when the folder last answered the health check, epoch ms; NULL until it first did. The
    -- Drives page says "last seen 2 days ago" for a drive that went away.
    last_seen_ms INTEGER
);

-- The same folder cannot be a source twice for one library, whatever its letter case.
CREATE UNIQUE INDEX data_source_path  ON data_source (kind, path COLLATE NOCASE);
CREATE INDEX        data_source_order ON data_source (kind, position, id);

-- ---------------------------------------------------------------------------------------------
-- Scan fingerprints: what the scanner last saw for every file it grouped
-- to a library item. A rescan compares the files on disk with these rows and only re-reads the
-- ones whose size, modification time or file identity differ.
--
-- A fingerprint is derived data, not a library row. It is keyed by the file's path (ignoring
-- case, like the file systems the app runs on) and never references an item, so removing an
-- item, a data source or the whole table loses nothing: the next rescan simply sees the files as
-- new and falls back to the sizes and times stored on `media_file`.

CREATE TABLE fingerprint (
    path         TEXT    NOT NULL PRIMARY KEY COLLATE NOCASE,
    -- the data-source root the file was found under, as stored in `item.data_source`
    data_source  TEXT    NOT NULL COLLATE NOCASE,
    size_bytes   INTEGER NOT NULL DEFAULT 0,
    modified_ms  INTEGER NOT NULL DEFAULT 0,
    -- inode (with device) on Unix; empty where the platform does not expose it cheaply
    file_id      TEXT    NOT NULL DEFAULT '',
    -- when the row was last written; an unchanged file never rewrites its row
    recorded_ms  INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

CREATE INDEX fingerprint_source ON fingerprint (data_source, path);

-- ---------------------------------------------------------------------------------------------
-- Videos that media info could not be read from. The mark names the file by folder
-- and name and records the size and modification time it was tried at. A video with a mark that
-- still matches is not listed as pending and not tried again at every launch (each try can take
-- up to two minutes); a changed file no longer matches, so it is tried again. Only "Reload media
-- info" removes a mark by hand.
--
-- Derived data, like the fingerprints: no item references it, and losing it only costs one more
-- failed try per file.

CREATE TABLE media_unreadable (
    directory    TEXT    NOT NULL COLLATE NOCASE,
    filename     TEXT    NOT NULL COLLATE NOCASE,
    size_bytes   INTEGER NOT NULL DEFAULT 0,
    modified_ms  INTEGER NOT NULL DEFAULT 0,
    -- the short reason shown in the Activity drawer
    reason       TEXT    NOT NULL DEFAULT '',
    marked_ms    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (directory, filename)
) WITHOUT ROWID;

-- ---------------------------------------------------------------------------------------------
-- What each after-scrape step did for each item.
-- One row per (item, step), replaced on every run of the step. The lifecycle (stage and the
-- explanation lines) and the ratings strip read it, so a step that was skipped, found nothing or
-- needed a key is visible instead of silent. Steps include `scrape`, artwork, ratings,
-- collections, and `ratings:<source>` for one rating source of the ratings step.
-- Derived data: losing a row costs one explanation line until the step runs again.

CREATE TABLE item_step_outcome (
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    step                  TEXT    NOT NULL CHECK (step <> ''),
    status                TEXT    NOT NULL
                              CHECK (status IN ('done', 'unchanged', 'skipped', 'needs_key', 'not_found', 'failed')),
    -- one plain sentence; empty when there is nothing to add
    reason                TEXT    NOT NULL DEFAULT '',
    at_ms                 INTEGER NOT NULL,
    PRIMARY KEY (item_id, step)
) WITHOUT ROWID;

-- The Activity "Needs attention" groups read failures and missing keys across items.
CREATE INDEX item_step_outcome_status ON item_step_outcome (step, status)
    WHERE status IN ('failed', 'needs_key');

-- ---------------------------------------------------------------------------------------------
-- The organize-state cache: for each movie and show,
-- whether its folder and file names already match the active naming profile. A background job
-- runs the renamer planner as a dry run per item and stores the result here, so the lifecycle
-- (stage Renamed, the Rename pending flag, rename conflicts) and the pipeline counts need no
-- planning at read time.
--   tidy       the names already match the profile
--   pending    a rename to the profile's names would change something
--   conflict   a rename would collide with another title or file (the reason says how)
--   locked     the title is locked, so nothing is planned
--   unplanned  it cannot be planned now (files missing, folder offline, a template problem)
-- `profile_key` identifies the profile the row was computed for. A row with another key is stale
-- and reads as "checking" until the job recomputes it.
-- Derived data: losing a row costs one recompute.

CREATE TABLE item_organize_state (
    item_id               INTEGER PRIMARY KEY REFERENCES item (id) ON DELETE CASCADE,
    state                 TEXT    NOT NULL
                              CHECK (state IN ('tidy', 'pending', 'conflict', 'locked', 'unplanned')),
    reason                TEXT    NOT NULL DEFAULT '',
    profile_key           TEXT    NOT NULL,
    computed_ms           INTEGER NOT NULL
);

CREATE INDEX item_organize_state_state ON item_organize_state (profile_key, state);

-- ---------------------------------------------------------------------------------------------
-- The missing/duplicate review: a row the user chose
-- to keep stays in the library and leaves the review list. `reason` is the kind of problem it
-- was kept for (`missing`, `offline`, `duplicate`, `superseded`); a row that later has another
-- problem is listed again. The row is never removed by this table, and removing the item removes
-- the entry with it.

CREATE TABLE review_kept (
    item_id               INTEGER PRIMARY KEY REFERENCES item (id) ON DELETE CASCADE,
    reason                TEXT    NOT NULL CHECK (reason <> ''),
    kept_ms               INTEGER NOT NULL
);

-- ---------------------------------------------------------------------------------------------
-- One row per item of an organize batch (safety review).
--
-- The rename journal has a row per file operation, written before the operation. A title is
-- several operations (the folder, the video, the NFO, the artwork, the subtitles), so a process
-- that stops *between* two of them leaves every row settled and the title half renamed, with
-- nothing in the journal to say so. This row is written before the title's first operation and
-- settled after its last one, so a stop anywhere inside a title is found at the next start.
--   running      the title is being renamed (or the process stopped while it was)
--   done         every operation of the title ran
--   rolled_back  an operation failed and the ones before it were taken back
--   unfinished   the title is half renamed: the process stopped (start-up recovery turns
--                `running` into this), or a roll-back could not take everything back.
--                Activity offers Resume and Roll back.
--   abandoned    the user rolled the title back or undid the batch instead of resuming
-- `plan` is what the executor was given for the title (its batch item and clean-up options, as
-- JSON), so Resume runs exactly that again: steps already on the disk count as done.

CREATE TABLE rename_item_mark (
    batch_id              TEXT    NOT NULL,
    item_key              TEXT    NOT NULL,
    state                 TEXT    NOT NULL DEFAULT 'running'
                              CHECK (state IN ('running', 'done', 'rolled_back', 'unfinished', 'abandoned')),
    plan                  TEXT    NOT NULL DEFAULT '',
    started_ms            INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    finished_ms           INTEGER,
    PRIMARY KEY (batch_id, item_key)
) WITHOUT ROWID;

CREATE INDEX rename_item_mark_open ON rename_item_mark (state)
    WHERE state IN ('running', 'unfinished');

-- ---------------------------------------------------------------------------------------------
-- The indexers of the managed indexer service, as the last provisioning left them (Torrent
-- search).
--
-- The indexer service runs many public indexers. Provisioning enables the ones that pass a
-- connection test and deletes the ones that do not, so a search never waits for a broken one. This
-- table remembers what it found, for the Torrent sources page and for the sources a search asks:
--   working       passed the test; searches use it
--   needs_solver  the site shows a browser challenge and no challenge solver is set
--   unreachable   the site did not answer (connection, DNS, TLS or the 60 s limit)
--   error         anything else, with the reason in `reason`
--   off           the person switched it off; it is deleted from the service and never re-enabled
-- `checked_at` is epoch milliseconds. `user_off` survives every provisioning.

CREATE TABLE torrent_indexers (
    id         TEXT    PRIMARY KEY,
    name       TEXT    NOT NULL,
    state      TEXT    NOT NULL
                   CHECK (state IN ('working', 'needs_solver', 'unreachable', 'error', 'off')),
    reason     TEXT,
    checked_at INTEGER NOT NULL DEFAULT 0,
    user_off   INTEGER NOT NULL DEFAULT 0 CHECK (user_off IN (0, 1))
) WITHOUT ROWID;

-- ---------------------------------------------------------------------------------------------
-- History: one entry per batch that changed the user's files or
-- titles, kept across restarts and never removed automatically.
--   kind        rename, move, delete (to the Recycle Bin / Trash), delete_permanent, artwork,
--               match
--   at          epoch milliseconds
--   detail_json what the History page shows under the summary and what Undo needs (the binned
--               items of a delete, counts of a rename)
--   batch_ref   the rename_journal batch of a rename or move (the journal stays the source of
--               truth for the file steps)
--   undo_state  available, partial (some titles undone), undone, none (cannot be undone)
-- history_title lists the titles an entry touched with their old and new names or paths.
-- `item_id` has no foreign key: an entry outlives the rows it names (a permanent delete).
-- ---------------------------------------------------------------------------------------------
CREATE TABLE history_entry (
    id                    INTEGER PRIMARY KEY,
    kind                  TEXT    NOT NULL
                              CHECK (kind IN ('rename', 'move', 'delete', 'delete_permanent', 'artwork', 'match')),
    at                    INTEGER NOT NULL,
    summary               TEXT    NOT NULL DEFAULT '',
    detail_json           TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(detail_json)),
    batch_ref             TEXT,
    undone_at             INTEGER,
    undo_state            TEXT    NOT NULL DEFAULT 'available'
                              CHECK (undo_state IN ('available', 'partial', 'undone', 'none'))
);

CREATE INDEX history_entry_at    ON history_entry (at, id);
CREATE INDEX history_entry_kind  ON history_entry (kind, at, id);
CREATE INDEX history_entry_batch ON history_entry (batch_ref) WHERE batch_ref IS NOT NULL;

CREATE TABLE history_title (
    entry_id              INTEGER NOT NULL REFERENCES history_entry (id) ON DELETE CASCADE,
    item_id               INTEGER NOT NULL,
    title                 TEXT    NOT NULL DEFAULT '',
    before                TEXT    NOT NULL DEFAULT '',
    after                 TEXT    NOT NULL DEFAULT '',
    state                 TEXT    NOT NULL DEFAULT 'done' CHECK (state IN ('done', 'undone', 'left')),
    PRIMARY KEY (entry_id, item_id)
);

CREATE INDEX history_title_item ON history_title (item_id, entry_id);
