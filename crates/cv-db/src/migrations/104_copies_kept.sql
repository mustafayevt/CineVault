-- "Keep both" on the Compare copies page: the user keeps every current copy of a movie. One
-- decision is one `set_id` with every member it covered; a group stays kept while all its current
-- members belong to one set, so a new copy opens it again. A removed title takes its own rows
-- with it. This table holds decisions only; no title is ever removed through it.
CREATE TABLE copies_kept (
    set_id                INTEGER NOT NULL,
    item_id               INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    kept_ms               INTEGER NOT NULL,
    PRIMARY KEY (set_id, item_id)
) WITHOUT ROWID;

CREATE INDEX copies_kept_item ON copies_kept (item_id);
