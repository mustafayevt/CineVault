## Context

`torrents::wishlist` judges every entry that is not done against the library on each list and check (`library_meets`) and marks it done as soon as a copy meets the criteria. Criteria are JSON (`WishCriteria`, serde defaults), so a new field needs no migration; remembering what the library held when the entry was saved does.

## Decisions

### Count the copies that meet the criteria, and remember the count at save

A new column `torrent_wishlist.held_copies` (integer, default 0) holds how many library copies met the criteria when the entry was added, last edited, or told to keep looking. Judging counts them again:

- a movie: every library copy with its TMDB ID that has a video, counted one by one;
- a season: 1 when every aired episode is held and the quality most of them share meets the criteria, else 0;
- an episode: 1 when that episode is held and meets, else 0;
- next new episode: always 0 (it is never done, as today).

The entry is done when the count is **above** `held_copies`. When the count is **below** it (a copy was deleted, or its media info changed), `held_copies` is lowered to the count, so a later arrival still closes the entry. Existing entries get 0, which keeps today's behaviour for them; entries already done stay done.

Alternatives: a boolean "held at save" closes nothing when a second copy of a movie arrives, which is exactly the case of #68; remembering copy identities (item ids) is precise but needs a match table and breaks when a rename re-creates items. A count is enough for the cases that matter and lives in one column.

### Keep looking resets the baseline

"Keep looking" is a command, not an edit: it clears `done_ms` and the last check and sets `held_copies` to the current count. Matches that are new or seen go (as on an edit), dismissed and sent ones stay. Editing an entry recomputes `held_copies` the same way, so editing a done entry also re-opens it.

### The held copy is computed, not stored

`WishEntry` gains `held: Option<LibraryCopy>`: the best movie copy, the season's shared quality when every aired episode is held, or the episode's copy. The editor asks for it with `torrent_wishlist_held(kind, tmdbId, scope)` as the scope changes (it reads only the library), and the card uses the entry's own. The line names resolution, source and HDR, the words the criteria use; unknown parts are left out, and with nothing known it reads "You already have this".

### Smallest size

`WishCriteria.min_size: Option<u64>` next to `max_size`; `met_by_release` checks both, and an unknown size fails either. The DTO refuses a negative value and a smallest above the largest; the editor shows the same problem before saving.

## Risks / Trade-offs

- [A copy replaced by one of the same quality does not close an entry] → stated as out of scope; Remove or Keep looking cover it.
- [Counting copies on every list] → the list already reads every copy per entry for the done rule; the count reuses it.

## Data safety

Nothing here writes, moves or deletes media files; only the Wishlist tables change. The migration adds a column with a default, so it cannot fail on existing rows, and a crash between list and write leaves the entry to be judged again on the next list.
