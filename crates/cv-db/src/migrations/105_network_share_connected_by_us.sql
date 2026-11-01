-- Whether CineVault itself made the system's connection to a share (1) or found it there, made by
-- the user (0). CineVault only ever disconnects a connection it made: never the user's own drive
-- letter or file-manager mount of the same share. Set when the system's tool actually connected,
-- in this run or an earlier one; cleared when CineVault learns the connection is gone.
ALTER TABLE network_share
    ADD COLUMN connected_by_us INTEGER NOT NULL DEFAULT 0 CHECK (connected_by_us IN (0, 1));
