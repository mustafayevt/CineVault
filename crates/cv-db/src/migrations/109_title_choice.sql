-- The choice of title of a movie or a show: with `use_original_title` on, `title` holds the
-- original title and the title in the metadata language waits in `localized_title`, so switching
-- back restores it. Every title a library already holds keeps its localized title (0, '').
ALTER TABLE item ADD COLUMN use_original_title INTEGER NOT NULL DEFAULT 0
    CHECK (use_original_title IN (0, 1));
ALTER TABLE item ADD COLUMN localized_title TEXT NOT NULL DEFAULT '';
