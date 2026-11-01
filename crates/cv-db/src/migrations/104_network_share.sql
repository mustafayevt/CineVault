-- Network shares connected with a login of their own (the Network share page). No password is ever
-- stored here: a remembered one lives in the system credential store as `network-share.<id>`.
-- `data_source` is unchanged: a library folder belongs to a share when its path is the share's
-- `local_root` or lies below it, compared with the platform's path rules.
CREATE TABLE network_share (
    id                 INTEGER PRIMARY KEY,
    server             TEXT    NOT NULL CHECK (server <> ''),
    share              TEXT    NOT NULL CHECK (share <> ''),
    -- the user name of the login, with an optional DOMAIN\ part; shown, so not a secret
    user               TEXT    NOT NULL DEFAULT '',
    -- where the share's files appear: \\nas\movies on Windows, the mount folder on macOS, the
    -- desktop's user-mount path on Linux
    local_root         TEXT    NOT NULL CHECK (local_root <> ''),
    -- whether the password is in the credential store (1) or only in memory until CineVault quits (0)
    remember           INTEGER NOT NULL DEFAULT 1 CHECK (remember IN (0, 1)),
    created_ms         INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    -- when the share last connected, epoch ms; NULL until it first did
    last_connected_ms  INTEGER,
    -- why the last connect failed, as a code such as `wrong_login` (never a message holding user
    -- input); NULL after a success
    last_error         TEXT,
    -- when the last library folder on the share was removed; NULL while a folder uses it
    unused_since_ms    INTEGER
);

-- One row per place a share appears, whatever its letter case.
CREATE UNIQUE INDEX network_share_root ON network_share (local_root COLLATE NOCASE);
