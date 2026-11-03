# Changelog

What changed in each version of CineVault, newest first, in plain words. The app shows this file under Settings › Updates › Release notes, and after an update it shows the notes of the version you just got. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.6.0] - unreleased

### Added

- Torrents: the Wishlist's limits take a smallest size as well as a largest, so the size works as a range (for example 20–60 GB) and a check skips small re-encodes that pass the resolution.
- Activity: History is now called Activity. While something needs you, its top shows Needs attention: a move or rename that did not finish, with where it was going, how far it got ("8 of 10 files copied, 15 GB of 86 GB"), why it stopped and Resume and Roll back, and the warnings and errors from background work (a rejected API key, artwork that could not be saved…), each with its fix and Dismiss. History stays below it. The Activity icon in the sidebar shows a dot while something is there.
- Moves: a move cut short because CineVault was closed carries on by itself the next time CineVault opens, from where it stopped, when both drives are there. A toast says so, and the progress with Pause and Cancel shows in the sidebar and in Activity. A move that stopped on an error, such as a full or unplugged drive, waits in Activity for you.
- Move to: for a title held by a move that did not finish, the menu says how far that move got and offers Resume that move, Roll it back and Open in Activity.

### Changed

- Torrents: a title you already have no longer goes straight to Done on the Wishlist. Its entry says what you have ("You already have this: 1080p · WEB-DL", linking to the title) and keeps being checked; it's done only when a copy that meets what you asked for arrives after you added or edited it. A done entry offers Keep looking.

### Fixed

- Moves: "Resume it or roll it back in Activity first" pointed to a page that did not exist, so a stopped move could not be resolved and its title could not be moved again. A stopped move now also shows a toast with Open Activity.
- Settings: clicking a section in the index, especially the last one, no longer scrolls the whole window and pushes the index off screen or leaves empty space below the last card. Only the settings scroll, and only as far as they go.
- Moves to another drive: resuming a move that stopped in the middle of a large file (CineVault was closed, or the drive filled up) now carries on from where the file stopped instead of copying it again from the start. What was already copied is checked against the original first, and the whole file is still checked once it is done.
- Move to: a title waiting on a move that did not finish now shows its size instead of "0 B".

## [1.5.0] - 2026-11-03

### Added

- Browser mode: CineVault can be added to a phone's home screen (Safari › Share › Add to Home Screen on iPhone, Install app in Chrome on Android) and opens full screen with its own icon. You sign in once more inside the home-screen app.
- Browser mode on phones and tablets: below 1024px wide the sidebar opens as a drawer from the menu button at the top left, next to Back and the search, every page fits the width without scrolling sideways, buttons and menu items are bigger on touch screens, and a long press opens the menu that a right-click opens on a computer.
- Browser mode over HTTPS: CineVault starts faster from what it already downloaded, and says "Can't reach CineVault" with Try again when the server doesn't answer, instead of the browser's error page.
- Drives: a Refresh button next to "Updated …" reads every drive again at once, for example after copying files outside CineVault or plugging a drive in.
- Copies: the Movie page of a movie you have more than once shows a tab for each copy at the top of its Technical card ("2160p DV", "1080p"), the best one marked. A tab switches the page to that copy: its file, NFO, Rename history, Play and Move. The Copies row at the end of the card offers Keep only this one, Delete this copy and Compare. It replaces the "You have 2 copies · Compare" line.
- Torrents: when you search a title you already have, a "You have this" card shows what you have: each copy of a movie (resolution, HDR, codec, audio, source, edition, size), or the seasons of a show and the episodes you're missing ("You have S01–S02 · missing S02E04–E10"). It replaces the thin "Yours:" line. Search anyway folds it into one line.
- Torrents: the Better, Same as yours and Worse tag on a release now says why when you point at it ("Better: 2160p Dolby Vision vs your 1080p SDR"), and clicking it opens Compare with yours: your copy and the release side by side, the differences marked, ending in a plain verdict like "Better picture, better audio, 2.5× the size". The Latest releases details panel shows the same comparison for a movie you have.
- Torrents: an "Only better than mine" chip hides the releases that are no better than your copy. For a show, releases are tagged Fills a gap, In your library or Better, and a "Fills a gap" chip lists only the ones with episodes you're missing.
- Movie and show pages: the ⋯ menu has Find a better copy (movies) and Find missing episodes (shows), which open Torrents with the title picked and the matching chip on.
- Torrents: a Wishlist, next to Search at the top of the Torrents page. Add a movie, a season, an episode or "the next new episode" of a show with the kind of release you want: the lowest resolution, the sources (Remux, Blu-ray, WEB-DL, WEBRip), HDR only, a largest size and the fewest seeders. Add titles with Add on the Wishlist, from a torrent search, from a film of a collection you don't have, or from the ⋯ menu of a Movie or Show page.
- Torrents: CineVault checks the Wishlist when it opens and every hour while it stays open, each title at most every 6 hours, or at once with Check now. New releases that match show as a count on Torrents in the sidebar and as a toast, each with Send to torrent app and Dismiss. Nothing is sent without you. A title is marked done once your library has a copy that meets what you asked for.

### Fixed

- Torrents: Compare with yours from a release's right-click menu sometimes closed again at once.
- Drives: clicking a drive in the sidebar now opens the Drives page on that drive, also when the page is already open, and the sidebar shows which drive is chosen.
- Rename no longer gives a downloaded info file (a plain-text `.nfo` about the video, named like it) the movie's NFO name. Before, CineVault could then never write its own NFO for that movie, and when the movie already had one, the info file could push it into the trash. Now a rename sets the info file aside with the other leftovers (Undo brings it back), so only CineVault's NFO stays with the title.

## [1.4.0] - 2026-11-02

### Added

- Network shares: add a library folder from a NAS with its user name and password (Settings › Library folders › Network share…). CineVault keeps the password in your system's credential store, reconnects at start, and says why a share is offline ("Wrong password", "Server not reachable") with Connect… to fix it. It never disconnects a connection you made yourself. The server installers set shares up for browser mode.
- Copies: if you have the same movie twice (say a 4K and a 1080p download), the Copies filter shows them, Compare puts them side by side, and Keep this one deletes the other the usual way. Keep both, and Rename gives each its own name, like "- 2160p" and "- 1080p". When two copies may be one file reached two ways (a drive and its network share), Compare does not offer to delete either.
- CineVault keeps a checked backup of your library every day (the last seven), so History and Undo can be brought back, and tells you if backups keep failing.

### Changed

- One Delete everywhere. It follows Settings › Deleted files; the separate Delete permanently item is gone, Shift+Delete does the same as Delete, and a permanent delete asks Yes or No instead of making you type DELETE.
- The Delete confirmation shows each title's folder under its name, so two titles with the same name can be told apart.
- A library folder that is already in your library under another path (a network share, a mapped drive or a link) can no longer be added a second time.
- IMDb is no longer read from its web site, which kept asking for a human check. IMDb IDs come with every TMDB match, IMDb ratings from IMDb's own ratings file, and with an OMDb key OMDb adds IMDb data to Scrape.
- Browsers stay signed in when the server restarts or the desktop app updates. Switching Browser access off, changing its port, signing out or choosing a new password still signs them out.

### Fixed

- Linux and browser mode: titles on a network share mounted at a folder such as `/media/nas`, or in a Docker install, no longer show as missing. They are available again, and Rename and Move work on them.
- Scrape: a title whose name already ends in its year, such as `Example Movie (2003)`, is no longer searched with the year twice, which found nothing.
- Rename to another drive: the copy is written to the disk before the original is removed, so a power cut during a rename can no longer lose the file.
- Rename to another drive: a folder that holds a link to another folder is no longer moved, so files outside the title can't be touched; move such a title by hand.
- Linux installer: updating with the server tarball no longer ends with a list of "Permission denied" messages about its temporary folder.
- Windows Docker installer: Update, Change folders and Remove show the right data folder again.
- Docker: the container starts faster with a large library and keeps the groups your NAS folders need.

### Security

- A browser can no longer change the update channel, the update signing key, a tool's path or the video player. These are changed at the computer that runs CineVault; on a server, a tool's path goes in `settings.json` while CineVault is stopped. The custom trash folder and the collection folder must be inside the folders the server may browse.
- A request that comes through a reverse proxy or tunnel is treated as outside your home network unless the proxy is listed in `CINEVAULT_WEB_TRUST_PROXY`, so a stranger cannot reach the welcome page or a password reset through it, and "Only devices on my home network" refuses it.
- A flood of wrong passwords from the internet no longer stops you from signing in from your home network, and checking passwords no longer slows the server for everyone else.
- No other website can show CineVault inside a frame.

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
