-- How many library copies met an entry's criteria when it was added, last edited or told to keep
-- looking. The entry is done only when more copies meet them than that, so a title the library
-- already held stays on the Wishlist and is checked. It is lowered when copies leave, so a later
-- arrival still counts. Existing entries start at 0, which judges them as before.
ALTER TABLE torrent_wishlist ADD COLUMN held_copies INTEGER NOT NULL DEFAULT 0;
