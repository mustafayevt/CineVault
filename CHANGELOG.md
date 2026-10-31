# Changelog

What changed in each version of CineVault, newest first, in plain words. The app shows this file under Settings › Updates › Release notes, and after an update it shows the notes of the version you just got. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.3.0] - 2026-10-31

### Added

- Browser mode installs with one command: on Linux it installs the server package and starts it as a service, on Windows and macOS it sets CineVault up in Docker Desktop. Running the same command again updates CineVault, changes its folders or removes it. There is also an Unraid template and a simpler Docker Compose file.
- Torrents: the page now opens with the latest releases from your torrent sources, in the same list as a search, filling in as each source answers. Choose All, Movies or TV, sort by newest, seeders, size or quality, filter by resolution, and camera and TS copies are hidden unless you switch "Hide CAM and TS" off. Scroll down for more; a source that has nothing more says so. Type a title to search as before, and clear the field to come back to the list.
- Torrents: releases of titles you already have say "In your library", or "Better than your copy" when they beat the movie you hold. This is worked out from your library alone.
- Torrents: click a release (or its Details button) and a side panel slides in and looks the title up on TMDB: poster, plot, rating and links to TMDB, IMDb, Letterboxd and Trakt. If the match looks wrong it says so and lets you pick another title. It also lists the title's other releases and can search all of them. Without a TMDB key it shows what the release name says.
- Browser access: turn it on in Settings › Browser access and open your library from a phone, a TV or another computer on your home network, with no Docker and no second install. The CineVault you already run serves it, so the window and the browser see the same library at the same time. The section shows the address to open with a Copy button and a QR code, lets you choose the port, and lists the signed-in browsers with Sign out for each or for all.
- The password for Browser access is created and changed in Settings, right at the computer. Turning Browser access off closes the port and signs every browser out.
- Only devices on your home network can reach Browser access unless you turn that off.
- Two options for Browser access: keep CineVault running in the tray when the window is closed, and start CineVault when you sign in.
- Server: "Forgot the password?" on the sign-in page writes a one-time PIN to `password-reset-pin.txt` in the data folder. Enter it with a new password from a browser on your home network; it works once, for 15 minutes, and signs every other browser out.

### Changed

- Server: the first-run welcome no longer closes after 30 minutes. It stays until a password exists, but only browsers on the same computer or the home network can use it.

### Fixed

- A password set with `cinevault-server passwd` as root (with sudo, or `docker exec` without `-u`) now works right away instead of only after a restart.
- The server packages and tarball are attached to every release again; 1.2.0 was missing them at first.
- Removing the server `.deb` no longer takes an empty `/usr/local/bin` folder with it.
- Torrents: the "Send to torrent app" button at the end of each row was cut off ("Send to t…"). It now reads "Send" and always fits; its tooltip still says what it does.
- The Move to menu is clean and dependable again. Each drive is a row of its own with its free space, the folder's path shortened in the middle (hover it for the whole path) and the free space after the move. A drive that cannot take the titles now says why on its row, such as "Not enough space: needs 71.4 GB, 1.02 GB free.", instead of only looking greyed out, and the menu no longer shows stray scrollbars.
- A click outside the Move to menu now closes it and does what you clicked, so selecting a title no longer takes two clicks. Pressing "Move to…" again closes the menu, Esc closes it and keeps your selection, and the menu opens above the selection bar without covering it, also in a small window.
- The Move to menu shows placeholder rows while it measures free space, says when there is no other folder to move to with a way to Settings › Library folders, and works fully from the keyboard.

## [1.2.0] - 2026-10-30

### Added

- Browser mode (beta): CineVault can run on a home server next to the media files and be used from any browser: sign in, browse the library and the title pages, scan, scrape and fix a match, choose artwork, rename with the preview and undo, and read the History. The server is a Linux program with no desktop stack, shipped as a container image, a `.deb`/`.rpm` and a tarball; the `cinevault` command line is included.
- One password for the server, created in the first-run welcome, with a 30-day session, "sign out every session" and a `cinevault-server passwd` recovery command.
- The server keeps provider keys in an encrypted `secrets.enc` and takes the master key from a Docker secret or a systemd credential. The desktop app keeps using the operating-system credential store.
- The server and the desktop app share one library safely: the server holds a `library-owner.json` lock, the desktop app refuses that folder, and the command line exits with code 2 instead of writing behind the server's back.
- Browser playback of the server's files with seeking; the desktop player choice stays in the desktop app.
- A Server section in Settings with the address, version, session count, key state, password change and "sign out every other session".

### Changed

- Deleting a title that shares its folder with other titles, or sits directly in a library folder, now also removes the subtitles, posters, NFO files, trailers and other files named after it, even ones added since the last Scan. Deleting an episode does the same for its files. Files of the other titles stay, and Undo brings them back.

### Fixed

- Ultra HD Blu-ray folders and images with Dolby Vision now show Dolby Vision (profile 7) next to HDR10. The disc's second video layer was taken for a plain HDR10 stream, so only HDR10 was shown.
- Blu-ray folders and images now show the language of every audio and subtitle track, read from the disc's own track list.
- Dolby Digital Plus tracks that were shown as AC3 (in Blu-ray files and some MKV files) are now called E-AC-3, with Atmos when they have it.
- DTS:X tracks with height speakers now show their real layout, such as 7.1, instead of 11.1.
- Interlaced videos stored as separate fields now show their frame rate (29.97 instead of 59.94).
- After the update, CineVault reads the media info of Blu-ray discs, disc images, Blu-ray files and the videos above once more, so they show the corrected details.
- Linux: the AppImage, .deb and .rpm now start on Ubuntu 22.04, Debian 12, Linux Mint 21 and other systems of that age. 1.1.0 was built on a newer system and needed a newer C library (glibc 2.38) than they have, so it closed right away.

## [1.1.0] - 2026-10-28

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
