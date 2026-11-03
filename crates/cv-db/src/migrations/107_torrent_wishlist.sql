-- The torrent Wishlist: titles the user wants a release of, with the kind of release they are
-- after, and the releases the checks found for them. Nothing here is library data: a title or
-- media file is never touched through these tables, and only the user removes an entry.
--   kind           movie or show
--   scope          movie, season:<n>, episode:<s>:<e> or next (the next new episode)
--   criteria_json  the criteria as the app writes them (resolution, sources, HDR, size, seeders)
--   added_ms, checked_ms, done_ms  epoch milliseconds; done_ms is set when the library holds a
--                  copy that meets the criteria, and the entry is no longer checked
--   check_error    what the last check could not do: `no-source` (no source answered),
--                  `airs:<date>` or `no-new-episode` (a "next" entry waits), or a failure's words
--   target         what a "next" entry looked for at its last check, `S03E06`
CREATE TABLE torrent_wishlist (
    id             INTEGER PRIMARY KEY,
    kind           TEXT    NOT NULL CHECK (kind IN ('movie', 'show')),
    tmdb_id        INTEGER NOT NULL,
    title          TEXT    NOT NULL,
    year           INTEGER,
    poster_url     TEXT,
    scope          TEXT    NOT NULL,
    criteria_json  TEXT    NOT NULL,
    added_ms       INTEGER NOT NULL,
    checked_ms     INTEGER,
    check_error    TEXT,
    done_ms        INTEGER,
    target         TEXT
);

CREATE UNIQUE INDEX torrent_wishlist_title ON torrent_wishlist (kind, tmdb_id, scope);

-- The releases a check kept for an entry, one row per release (`key`, the search's merge key).
-- Its quality is read from its name again when it is shown.
--   state  new (not looked at yet), seen, sent (handed to the torrent app) or dismissed (hidden,
--          and never stored again for this entry)
CREATE TABLE torrent_wishlist_match (
    entry_id     INTEGER NOT NULL REFERENCES torrent_wishlist (id) ON DELETE CASCADE,
    key          TEXT    NOT NULL,
    name         TEXT    NOT NULL,
    size         INTEGER,
    seeders      INTEGER,
    magnet       TEXT,
    torrent_url  TEXT,
    sources      TEXT    NOT NULL DEFAULT '',
    found_ms     INTEGER NOT NULL,
    state        TEXT    NOT NULL DEFAULT 'new'
                     CHECK (state IN ('new', 'seen', 'sent', 'dismissed')),
    PRIMARY KEY (entry_id, key)
) WITHOUT ROWID;
