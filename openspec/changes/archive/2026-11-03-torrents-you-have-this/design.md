## Context

The title details command (`torrent_title_details`) returns a movie's largest video as `libraryCopy` and a show's held episodes as `heldEpisodes`. The release search compares each release with one library copy in `cv_torrent::rank::upgrade` (resolution, then source, then HDR) and sends the verdict as `upgrade`. A show item stores the provider's missing episodes (`missing_episodes`), so the gaps are known without a lookup.

## Decisions

- **One comparison.** The verdict stays in Rust (`upgrade`), used by the search and the Latest releases list alike. The UI never decides it again: the compare rows show both sides and mark the rows that differ; the tag, the tooltip's reason and the verdict sentence come from the backend verdict plus those rows. So they cannot disagree.
- **Best copy.** With several copies of a movie, the search compares with the best one in the order `upgrade` uses (resolution, source, HDR, then size). `libraryCopies` lists the copies best first, so the first is the compared one, and with several the card says "Releases are compared with the first."
- **Copy facts** come from the stored media info: codec from the video codec (HEVC, AVC, AV1…), audio from the hero audio track ("TrueHD Atmos 7.1"), edition from the movie's edition label, languages from the audio tracks. Nothing is read from disk.
- **Episode span on releases.** Each release carries `episodes`: what its name covers (one season's episode range, a season, a range of seasons, the whole show). A season covers the episodes the library knows for it (held and missing); a season the library knows nothing of gives no tag.
- **Show quality.** A show or season search compares with the quality most held episodes in scope share (ties go to the lower), so "Better" means better than what you mostly have.
- **The chip in the address.** `/torrents?title=movie:1&only=better` or `only=gaps`, so the title page entries and a reload keep the chip; choosing a chip updates the address.
- **Compare as a popover.** On the search page a row click selects and Enter or a double-click sends, so the compare block opens from the tag, which becomes a button. In the Latest releases panel it is a section of the panel.

## Risks

- Media info not read yet: the copy line shows what is known; the verdict already treats an unknown resolution as equal.
