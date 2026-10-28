# Changelog

What changed in each version of CineVault, newest first, in plain words. The app shows this file under Settings › Updates › Release notes, and after an update it shows the notes of the version you just got. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.1.0] - unreleased

### Added

- Spotlight search: type in the search box in the title bar and see matching titles at once, grouped, with your recent searches and "See all results".
- Torrents: type a title and pick it from the suggestions before any release is searched. The picked title stays as a chip you can change.
- Library table: drag a column header to move it, drag its edge to resize it (double-click to fit), or use Move left and Move right in the header menu. Widths and order are remembered.
- A Stage column with clear New, Scraped and Renamed tags.
- Reload media info: re-read the video and audio details of the selected titles (or the whole list) from the toolbar, the right-click menu, the selection bar or the title's ⋯ menu.
- Choose the poster size in the grid toolbar. It is remembered.
- Look at an artwork picture large in the artwork picker: press the expand button, double-click or press Space. Click or Esc closes it.
- Change artwork is now a button in the title page header.
- Optional Fanart.tv and TheTVDB keys in Settings › Providers and in the first-run setup, and a "needs a key" link in search that opens them.
- Release notes of an available update, shown before you update, and a Release notes view with every version in Settings › Updates.
- "What's new" after an update: a short card with the changes of the version you just installed.
- CineVault for Linux, as a beta for 64-bit systems: an AppImage, a .deb package for Debian and Ubuntu and an .rpm package for Fedora. Deleted titles go to the system Trash of their own drive, API keys are kept in the system keyring (GNOME Keyring, KWallet or KeePassXC), and the AppImage updates itself from inside the app.
- CineVault is now open source under the Apache-2.0 licence. The code, the issue tracker and the releases are at github.com/mustafayevt/CineVault, and contributions are welcome (see CONTRIBUTING.md).
- Settings › Naming: a Style control at the top fills the movie folder, movie file and TV episode templates from the same styles as the first-run setup. Editing a template switches it to Custom, and choosing a style over your own templates asks first.

### Changed

- Downloads, updates and the website moved to the project's own GitHub page. The app now looks for updates there first and uses the earlier releases page only when that cannot be reached. Installed copies update as before, and nothing needs to be done.
- Delete… is now the last item of a title's ⋯ menu, in red, after a divider.
- The date a title was added now comes from its video file's creation time, and titles that all got the date of the first scan are corrected once.
- Opening and closing an episode on a TV show page is smoother, and it stays instant when you have reduced motion on.
- The large artwork view now loads the full-size picture, so it is sharp: the small preview shows at once and the full picture fades in, with its real size under it.

### Fixed

- Settings › Naming: the TV episode example is no longer empty, and every template field now has its token buttons (the TV one also has Channels).
- A video ratio wider than its frame (for example 2.39:1 in a 16:9 file) is now reported instead of being cut to the frame.

## [1.0.1] - 2026-10-27

### Added

- Choose where deleted files go: the Recycle Bin, a trash folder on each file's own drive, a folder you pick, or nowhere (erased at once). Settings › Deleted files has the choice.
- Each drive's trash shows its size, with an Empty button. Emptying touches only CineVault's own folder, also inside a trash folder you chose.
- Show the exact HDR format (HDR10, HDR10+, Dolby Vision and so on) on the Technical card.
- A new landing page for CineVault 1.0.

### Changed

- Moving titles between drives is faster, and each move is still verified before the original goes to the trash.
- Search finds titles whatever the punctuation, spacing or accents, and forgives small typos.

## [1.0.0] - 2026-10-27

CineVault 1.0 is a new design and a fresh start. It does not read the library of an earlier build; your old library is set aside on the first start and you set up again with a short four-step welcome.

### Added

- A new desktop design: a floating sidebar with counts, drives and progress, a title bar with centred search, light and dark themes, calm motion, and keyboard use everywhere (Esc, Delete, F2 and the arrow keys).
- The Library as a poster grid or a table on one selection, with Movies, TV shows and Collections, filters for stage, drive, quality and problems, and sorting on every column with a column chooser.
- One-page Movie and TV show pages with the plot, facts, cast, ratings, a Technical card, and seasons with episodes that open in place. Open a cast member's IMDb page from the cast row.
- Scrape: a queue with a search-only pass, candidates scored with their evidence, "Accept all confident", and a page to pick another match for any title.
- Choose posters, backdrops and other artwork in an artwork picker, from the title header or for several titles at once, including collection artwork.
- Rename with word-by-word differences, conflicts shown in place and held-back titles, using templates with live examples in Settings › Naming.
- History: every rename, move, delete, artwork change and change of match is recorded, and you can undo the whole entry or a single title.
- Safe deletes: Delete sends titles to the Recycle Bin with Undo, and permanent delete needs you to type DELETE. History restores deleted titles without overwriting anything.
- Drives: a card per drive with its free space, the mix of quality, and when it was last seen. Move titles between drives with a Move to menu that shows the space before and after.
- Torrents: one search with a best match, resolution chips and a sortable table of releases.
- Settings in one page with sections for Providers, Naming, Folders, Torrent sources, Appearance and Updates. Changes apply at once.
- Update the app from within: CineVault checks, downloads and verifies the new version, and can install it when you close the app.
- Zoom the whole app with Ctrl/Cmd plus, minus and 0, or in Settings.
- Complete Kodi, Jellyfin and Emby NFO files, rewritten whenever a title changes.

### Changed

- One verb for each job everywhere: Scan, Scrape and Rename.
- The first scan of a fresh library reads local files only, so you see your titles at once.

## Earlier versions

Versions before 1.0 are described on the [releases page](https://github.com/mustafayevt/CineVault/releases).
