-- Forgiving title search: two folded keys per item (lower case, no diacritics or punctuation,
-- spaced and compact forms; see repo/search.rs). The keys are written whenever an item is saved;
-- this fills them for the titles a library already holds.
ALTER TABLE item ADD COLUMN search_title TEXT NOT NULL DEFAULT '';
ALTER TABLE item ADD COLUMN search_alt   TEXT NOT NULL DEFAULT '';
UPDATE item
   SET search_title = cv_search_key(title, english_title),
       search_alt   = cv_search_key(original_title, sort_title);
