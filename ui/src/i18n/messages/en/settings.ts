/** English strings of the settings area. Merged with the other areas in `../../catalog.ts`. */
export const settings = {
  // External tools page
  "tools.desktopOnly": "External tools can be managed in the desktop app.",
  "providers.desktopOnly": "Provider keys can be managed in the desktop app.",
  "providers.loadFailed": "CineVault could not read the provider list. {message}",
  "providers.testFailed": "The test did not pass.",
  "providers.removed": "Removed the {name} credentials.",
  "providers.restored": "Put the {name} credentials back.",
  "providers.saved": "Saved the {name} credentials.",
  "settings.page.title": "Settings",
  "settings.control.format.httpsUrl":
    "Enter a web address that starts with https://, or leave it empty. It was not saved.",
  "settings.control.format.url":
    "Enter a web address that starts with http:// or https://, with no user name or query, or leave it empty. It was not saved.",
  "settings.control.format.signingKey":
    "Enter the key as base64 (44 characters ending in =), or leave it empty. It was not saved.",
  "settings.source": "Settings",
  "settings.saveFailed": "A setting could not be saved",
  "settings.resetFailed": "A reset could not be saved",
  "settings.restoreFailed": "The previous settings could not be restored",
  "settings.changeUndone": "{reason} The change was undone.",
  "settings.unreadable": "Your settings could not be read",
  "settings.loadFailed": "Settings could not be loaded, so defaults are in use",

  // Settings: option labels
  "settings.appearance.theme.option.dark": "Dark",
  "settings.appearance.theme.option.light": "Light",
  "settings.appearance.theme.option.system": "System",
  "settings.appearance.motion.option.system": "System",
  "settings.appearance.motion.option.reduced": "Reduced",
  "settings.general.locale.option.system": "Match system",
  "settings.tv.episode-order.option.aired": "Aired",
  "settings.tv.episode-order.option.dvd": "DVD",
  "settings.tv.episode-order.option.absolute": "Absolute",
  "settings.advanced.size-units.option.base10": "Base 10 (GB)",
  "settings.advanced.size-units.option.base2": "Base 2 (GiB)",
  "settings.library.skip-patterns.title": "Folders to skip while scanning",
  "settings.library.skip-patterns.description":
    "Folder names the scan does not enter, separated by semicolons. Each entry is a regular expression, or a full folder path. A folder holding a .cinevaultignore file is always skipped.",
  "settings.library.skip-patterns.keywords":
    "exclude, ignore, skip, regex, folders, samples, extras, excluded",
  "settings.library.skip-nomedia.title": "Skip folders marked with .nomedia",
  "settings.library.skip-nomedia.description":
    "Leave out any folder that holds a .nomedia file, together with everything below it.",
  "settings.library.skip-nomedia.keywords": "exclude, ignore, marker, nomedia, hidden",
  "settings.library.vsmeta-artwork.title": "Use artwork from .vsmeta files",
  "settings.library.vsmeta-artwork.description":
    "Read the posters and backgrounds that a NAS stores in .vsmeta files next to the videos.",
  "settings.library.vsmeta-artwork.keywords": "synology, nas, vsmeta, artwork, poster, import",
  "settings.library.bad-words.title": "Words to remove from titles",
  "settings.library.bad-words.description":
    "Words stripped from file and folder names before the title is read, separated by commas. Each is a regular expression and matches in any case.",
  "settings.library.bad-words.keywords": "bad words, noise, release tags, parser, strip, title",
  "settings.metadata.country.title": "Certification country",
  "settings.metadata.country.description":
    "The country whose age rating is written when a title has several. Its release date is used only when the next setting asks for it.",
  "settings.metadata.country.keywords": "certification, age rating, mpaa, release date, region",
  "settings.metadata.local-release-date.title": "Use the release date of that country",
  "settings.metadata.local-release-date.description":
    "Off: the premiered date is the film's original release. On: it is the first release in the certification country.",
  "settings.metadata.local-release-date.keywords":
    "release date, premiered, local, country, region, original release",
  "settings.media.aspect-mode.option.fast": "Fast (4 short samples)",
  "settings.media.aspect-mode.option.default": "Standard (6 samples)",
  "settings.media.aspect-mode.option.accurate": "Accurate (30 samples)",
  "settings.media.aspect-multi-format.option.most-common": "The ratio seen most often",
  "settings.media.aspect-multi-format.option.higher": "The taller picture",
  "settings.media.aspect-multi-format.option.wider": "The wider picture",
  "settings.artwork.size.option.small": "Small",
  "settings.artwork.size.option.medium": "Medium",
  "settings.artwork.size.option.big": "Big",
  "settings.artwork.size.option.large": "Large",
  "settings.artwork.size.option.xlarge": "Extra large",
  "settings.library.watch-debounce-seconds.title": "Wait before scanning a changed folder",
  "settings.library.watch-debounce-seconds.description":
    "How long a watched folder must stay quiet before it is scanned again, so a copy in progress is not read half-finished. At least 5 seconds.",
  "settings.library.watch-debounce-seconds.keywords":
    "watch, delay, debounce, quiet, seconds, copy, rescan",
  "settings.media.aspect-mode.title": "Aspect ratio detection depth",
  "settings.media.aspect-mode.description":
    "How many short samples of the picture are measured. Only the accurate mode looks for a second format in the same film.",
  "settings.media.aspect-mode.keywords": "ratio, letterbox, samples, fast, accurate, ffmpeg",
  "settings.media.aspect-round-up.title": "Round the ratio up",
  "settings.media.aspect-round-up.description":
    "Record the next wider standard ratio instead of the nearest one.",
  "settings.media.aspect-round-up.keywords": "ratio, rounding, nearest, wider, standard",
  "settings.media.aspect-multi-format.title": "Films that change ratio",
  "settings.media.aspect-multi-format.description":
    "Which ratio to record when an accurate scan finds two formats in one film.",
  "settings.media.aspect-multi-format.keywords": "ratio, imax, two formats, most common, wider",
  "settings.renamer.keep-subfolders.title": "Keep sub-folders",
  "settings.renamer.keep-subfolders.description":
    "A renamed title stays in its sub-folder: Movies/Amadeus becomes Movies/Amadeus (1984). Styles with folders of their own, such as letter folders, start at the library folder.",
  "settings.renamer.keep-subfolders.keywords":
    "sub-folders, subfolders, category, keep, flatten, folder, structure, rename",
  "settings.files.deleted-mode.title": "Deleted and replaced files",
  "settings.files.deleted-mode.description":
    "Where Delete, a move's originals, rename leftovers and replaced artwork or NFO files go. Shift+Delete always erases, after its confirmation.",
  "settings.files.deleted-mode.keywords":
    "delete, deleted, recycle bin, trash, bin, erase, permanently, replaced, leftovers, originals",
  "settings.files.deleted-mode.option.bin": "Recycle Bin",
  "settings.files.deleted-mode.option.folder": "Trash folder on each drive",
  "settings.files.deleted-mode.option.custom": "Custom trash folder…",
  "settings.files.deleted-mode.option.erase": "Delete permanently",
  "settings.files.trash-folder.title": "Custom trash folder",
  "settings.files.trash-folder.description":
    "Files on this folder's drive go here. Files on other drives go to their own drive's trash folder, because moving them here would mean copying them.",
  "settings.files.trash-folder.keywords": "trash, folder, custom, deleted, recycle",
  "settings.delete.confirm.title": "Ask before deleting",
  "settings.delete.confirm.description":
    "Delete shows what goes where and asks first. When Deleted files is set to Delete permanently, Delete always asks.",
  "settings.delete.confirm.keywords": "delete, confirm, ask, confirmation, dialog, prompt",
  "settings.files.auto-empty-days.title": "Empty trash folders automatically",
  "settings.files.auto-empty-days.description":
    "Erases what has been in CineVault's own trash folders longer than this. Never touches the Recycle Bin.",
  "settings.files.auto-empty-days.keywords": "empty, trash, automatically, days, clean, erase, old",
  "settings.files.auto-empty-days.option.off": "Off",
  "settings.files.auto-empty-days.option.7": "After 7 days",
  "settings.files.auto-empty-days.option.30": "After 30 days",
  "settings.files.auto-empty-days.option.90": "After 90 days",
  "settings.renamer.cleanup-unwanted.title": "Move unwanted files to the trash",
  "settings.renamer.cleanup-unwanted.description":
    "After a rename, and with the Clean up unwanted files action, files of the types below go to the trash folder of the library folder. Nothing is deleted.",
  "settings.renamer.cleanup-unwanted.keywords":
    "unwanted, clean-up, cleanup, junk, txt, url, sfv, trash",
  "settings.renamer.unwanted-patterns.title": "Unwanted file types",
  "settings.renamer.unwanted-patterns.description":
    "Extensions such as .txt or .url, or a pattern for the file name, separated by commas.",
  "settings.renamer.unwanted-patterns.keywords":
    "unwanted, clean-up, cleanup, extensions, patterns, junk",
  "settings.artwork.poster-size.title": "Preferred poster size",
  "settings.artwork.poster-size.description":
    "The size of poster to pick when several are offered.",
  "settings.artwork.poster-size.keywords": "poster, size, resolution, images",
  "settings.artwork.fanart-size.title": "Preferred fanart size",
  "settings.artwork.fanart-size.description":
    "The size of background image to pick when several are offered.",
  "settings.artwork.fanart-size.keywords": "fanart, backdrop, size, resolution, images",
  "settings.artwork.other-resolutions.title": "Use other sizes when needed",
  "settings.artwork.other-resolutions.description":
    "Fall back to the other sizes when none matches the preferred size.",
  "settings.artwork.other-resolutions.keywords": "artwork, size, fallback, resolution",
  "settings.artwork.languages.title": "Preferred artwork languages",
  "settings.artwork.languages.description":
    "Language codes separated by commas, best first. Use none for images without text. Empty follows the metadata language, then English.",
  "settings.artwork.languages.keywords": "artwork, language, textless, poster, images",
  "settings.artwork.fanart-without-text.title": "Prefer fanart without text",
  "settings.artwork.fanart-without-text.description":
    "Put a background image with no lettering first.",
  "settings.artwork.fanart-without-text.keywords": "fanart, textless, backdrop, images",
  "settings.artwork.any-language.title": "Accept any language as a last resort",
  "settings.artwork.any-language.description":
    "When no image matches the preferred languages, take the best one in any language.",
  "settings.artwork.any-language.keywords": "artwork, language, fallback, images",
  "settings.advanced.log-level.option.error": "Errors only",
  "settings.advanced.log-level.option.info": "Normal",
  "settings.advanced.log-level.option.debug": "Detailed",

  // Settings: sections and settings (titles, descriptions and search keywords)
  "settings.general.locale.title": "Language and region",
  "settings.general.locale.description":
    "The interface is in English. Dates, times and numbers follow this region; Match system uses the operating system's setting.",
  "settings.general.locale.keywords":
    "date, time, number, language, locale, region, format, decimal",

  // Rename: the rename mode is decided in Automation
  "settings.appearance.theme.title": "Theme",
  "settings.appearance.theme.description":
    "Dark is the default. System follows the operating system.",
  "settings.appearance.theme.keywords": "dark, light, follow system, colour, color, appearance",
  "settings.appearance.motion.title": "Motion",
  "settings.appearance.motion.description":
    "Reduced replaces slide-ins, zooms and shimmer with a short crossfade. System follows the operating system.",
  "settings.appearance.motion.keywords":
    "reduced motion, animation, animations, crossfade, accessibility, appearance",
  "settings.appearance.zoom.title": "Size",
  "settings.appearance.zoom.description":
    "How large the whole app is drawn, from 80% to 150%. Ctrl and + or minus change it, Ctrl and 0 sets 100%; on macOS use Command.",
  "settings.appearance.zoom.keywords": "zoom, size, scale, bigger, smaller, text size, appearance",
  "settings.general.check-updates.title": "Check for updates",
  "settings.general.check-updates.description":
    "Look for a new version in the background. You see the release notes first, and nothing installs without you.",
  "settings.general.check-updates.keywords": "update, upgrade, version, release",
  "settings.general.update-interval.title": "Update check interval",
  "settings.general.update-interval.description": "How often CineVault looks for a new version.",
  "settings.general.update-interval.keywords": "update, upgrade, how often, daily, weekly, monthly",
  "settings.general.update-interval.option.daily": "Daily",
  "settings.general.update-interval.option.weekly": "Weekly",
  "settings.general.update-interval.option.monthly": "Monthly",
  "settings.general.update-channel.title": "Update channel",
  "settings.general.update-channel.description":
    "Leave empty to get updates from the official CineVault releases. To use another channel, enter the address of its signed update manifest; it must start with https.",
  "settings.general.update-channel.keywords":
    "update, channel, manifest, release, feed, url, address",
  "settings.general.update-public-key.title": "Update signing key",
  "settings.general.update-public-key.description":
    "The public key the manifest must be signed with, as base64. Leave empty to use the key built into this version of CineVault, when it has one.",
  "settings.general.update-public-key.keywords": "update, signature, signed, ed25519, public key",
  "settings.library.reset-new-on-update.title": "Reset NEW when updating folders",
  "settings.library.reset-new-on-update.description":
    "Clear every NEW marker when an update of your library folders starts, so only what that update finds is marked.",
  "settings.library.reset-new-on-update.keywords": "new, flag, badge, rescan, refresh, clear",
  "settings.media.store-mediainfo-xml.title": "Store MediaInfo XML next to the media",
  "settings.media.store-mediainfo-xml.description":
    "Save the video and audio details beside each video, so a slow network share is read only once.",
  "settings.media.store-mediainfo-xml.keywords": "sidecar, mediainfo, xml, cache, stream details",
  "settings.media.runtime-from-media.title": "Use the video length as the runtime",
  "settings.media.runtime-from-media.description":
    "Replace the scraped runtime with the length of the video file. Stacked parts are added up.",
  "settings.media.runtime-from-media.keywords": "duration, minutes, length, stacked, scraped",
  "settings.media.detect-aspect-ratio.title": "Detect the aspect ratio",
  "settings.media.detect-aspect-ratio.description":
    "Measure the visible picture without black bars after media info is read. Needs FFmpeg.",
  "settings.media.detect-aspect-ratio.keywords":
    "ratio, letterbox, black bars, crop, ffmpeg, widescreen",
  "settings.media.checksums-after-scrape.title": "Calculate checksums after a scrape",
  "settings.media.checksums-after-scrape.description":
    "Work out the CRC-32 or the picture hash of the video once a title is scraped. The picture hash needs FFmpeg.",
  "settings.media.checksums-after-scrape.keywords": "crc32, phash, hash, duplicate, verify",
  "settings.media.checksums-after-scrape.option.off": "Off",
  "settings.media.checksums-after-scrape.option.crc32": "CRC-32",
  "settings.media.checksums-after-scrape.option.phash": "Picture hash",
  "settings.media.checksums-after-scrape.option.all": "Both",
  "settings.library.remember-filters.title": "Remember filters",
  "settings.library.remember-filters.description":
    "Keep the search and filters of each library when CineVault restarts.",
  "settings.library.remember-filters.keywords": "persist, restore, search, chips, session",
  "settings.subtitles.language-style.title": "Language in subtitle file names",
  "settings.subtitles.language-style.description":
    "How the language is written in the name of a downloaded or dropped subtitle. The default follows the active naming profile.",
  "settings.subtitles.language-style.keywords": "subtitles, language code, iso, file name",
  "settings.subtitles.language-style.option.profile": "Same as the naming profile",
  "settings.subtitles.language-style.option.ISO3T": "ISO 639-2/T (eng, deu)",
  "settings.subtitles.language-style.option.ISO3B": "ISO 639-2/B (eng, ger)",
  "settings.subtitles.language-style.option.ISO2": "ISO 639-1 (en, de)",
  "settings.subtitles.language-style.option.LANG_EN": "English name (English, German)",
  "settings.subtitles.language-style.option.LANG_LOCALIZED": "Own name (English, Deutsch)",
  "settings.subtitles.language-style.option.ORIGINAL": "As the provider gives it",
  "settings.movies.collections.auto-group.title": "Group movies into collections",
  "settings.movies.collections.auto-group.description":
    "When a movie is scraped, put it in the collection the provider names, and make the collection if the library has none. Off leaves collections to Find collections and to you.",
  "settings.movies.collections.auto-group.keywords": "sets, franchise, series, boxset",
  "settings.tv.episode-order.title": "Episode order",
  "settings.tv.episode-order.description":
    "The numbering a show starts in when it is first scraped, if the provider offers it. Each show can be switched on its own page.",
  "settings.tv.episode-order.keywords": "aired, dvd, absolute, seasons, numbering",
  "settings.tv.specials.show.title": "Show specials",
  "settings.tv.specials.show.description":
    "List season 0 (specials, extras) with the other seasons. The Specials switch above a season list changes this too.",
  "settings.tv.specials.show.keywords": "season 0, extras, bonus",
  "settings.tv.season-art-fallback.title": "Use the show's artwork for seasons without any",
  "settings.tv.season-art-fallback.description":
    "A season with no poster or fanart of its own shows the show's instead. Files are not copied.",
  "settings.tv.season-art-fallback.keywords": "season poster, fallback, artwork",
  "settings.metadata.language.title": "Metadata language",
  "settings.metadata.language.description": "The language for titles, plots and genres.",
  "settings.metadata.language.keywords": "locale, translation, scraper",
  "settings.metadata.fallback-language.title": "Fallback language",
  "settings.metadata.fallback-language.description":
    "Used when a title or plot is missing in the metadata language.",
  "settings.metadata.fallback-language.keywords": "locale, translation, missing",
  "settings.matching.auto-accept-threshold.title": "Match automatically at",
  "settings.matching.auto-accept-threshold.description":
    "A title is matched without asking when its best candidate is at least this confident and no other candidate ties with it. Everything else waits in Needs review.",
  "settings.matching.auto-accept-threshold.keywords":
    "match, confidence, threshold, review, automatic, accept",
  "settings.metadata.scrape-do-not-overwrite.title": "Do not overwrite existing data",
  "settings.metadata.scrape-do-not-overwrite.description":
    "A scrape fills in empty fields only and keeps everything you already have.",
  "settings.metadata.scrape-do-not-overwrite.keywords": "scrape, overwrite, keep, empty, fill",
  "settings.metadata.scrape-fields-movie.title": "Movie fields to scrape",
  "settings.metadata.scrape-fields-movie.description":
    "The fields a scrape may write for movies. Locked fields and locked titles are never changed.",
  "settings.metadata.scrape-fields-movie.keywords":
    "scrape, fields, rating, artwork, lock, refresh",
  "settings.metadata.scrape-fields-show.title": "TV show fields to scrape",
  "settings.metadata.scrape-fields-show.description":
    "The fields a scrape may write for TV shows. Locked fields and locked titles are never changed.",
  "settings.metadata.scrape-fields-show.keywords": "scrape, fields, rating, artwork, lock, refresh",
  "settings.metadata.scrape-fields-episode.title": "Episode fields to scrape",
  "settings.metadata.scrape-fields-episode.description":
    "The fields a scrape may write for episodes. Locked fields and locked titles are never changed.",
  "settings.metadata.scrape-fields-episode.keywords": "scrape, fields, rating, thumbnail, lock",
  "settings.ratings.preferred-source.title": "Preferred rating",
  "settings.ratings.preferred-source.description":
    "The rating shown in the Rating column, used for sorting and written as the default rating in NFO files. A title without a rating from it shows the next available source, marked as a stand-in.",
  "settings.ratings.preferred-source.keywords":
    "rating, imdb, tmdb, letterboxd, sort, column, score",
  "settings.ratings.source.imdb-dataset": "IMDb ratings dataset",
  "settings.ratings.source.scraper": "The scraping provider",
  "settings.ratings.source.omdb": "OMDb",
  "settings.ratings.source.mdblist": "MDBList",
  "settings.ratings.sources-movie.title": "Rating sources for movies",
  "settings.ratings.sources-movie.description":
    "Where Fetch ratings looks for a movie's ratings. A source whose key is not set is skipped, and the rating says it needs a key until you add one in Providers.",
  "settings.ratings.sources-movie.keywords": "rating, omdb, mdblist, imdb, dataset, fetch",
  "settings.ratings.sources-show.title": "Rating sources for TV shows",
  "settings.ratings.sources-show.description":
    "Where Fetch ratings looks for a show's ratings. A source whose key is not set is skipped, and the rating says it needs a key until you add one in Providers.",
  "settings.ratings.sources-show.keywords": "rating, omdb, mdblist, imdb, dataset, fetch, series",
  "settings.ratings.fetch-after-scrape-movie.title": "Fetch ratings after scraping a movie",
  "settings.ratings.fetch-after-scrape-movie.description":
    "Adds the ratings of the rating sources for movies (Settings › Scraping and providers) to a movie as soon as it is scraped.",
  "settings.ratings.fetch-after-scrape-movie.keywords": "rating, automatic, scrape, fetch",
  "settings.ratings.fetch-after-scrape-show.title": "Fetch ratings after scraping a TV show",
  "settings.ratings.fetch-after-scrape-show.description":
    "Adds the ratings of the rating sources for TV shows (Settings › Scraping and providers) to a show as soon as it is scraped.",
  "settings.ratings.fetch-after-scrape-show.keywords": "rating, automatic, scrape, fetch, series",
  "settings.providers.prefer-original-title.title": "Prefer the original title",
  "settings.providers.prefer-original-title.description":
    "Show the title in its original language on cards, lists and detail pages, when the title has one.",
  "settings.providers.prefer-original-title.keywords": "native, foreign",
  "settings.metadata.nfo-format.title": "NFO format",
  "settings.metadata.nfo-format.description":
    "The flavour of the .nfo files written next to your media.",
  "settings.metadata.nfo-format.keywords": "kodi, jellyfin, emby, sidecar, metadata files",
  "settings.metadata.nfo-mediaportal-style.title": "MediaPortal NFO style",
  "settings.metadata.nfo-mediaportal-style.description":
    "Which MediaPortal layout movie NFOs use when the format is MediaPortal.",
  "settings.metadata.nfo-mediaportal-style.keywords":
    "mediaportal, moving pictures, myvideo, legacy, nfo",
  "settings.metadata.nfo-mediaportal-style.option.legacy": "Legacy",
  "settings.metadata.nfo-mediaportal-style.option.moving-pictures": "Moving Pictures",
  "settings.metadata.nfo-mediaportal-style.option.myvideo": "MyVideo",
  "settings.metadata.nfo-language.title": "NFO language",
  "settings.metadata.nfo-language.description":
    "The language of genre and spoken-language names in NFO files.",
  "settings.metadata.nfo-language.keywords": "nfo, genres, languages, translation, sidecar",
  "settings.metadata.nfo-certification.title": "Certification format",
  "settings.metadata.nfo-certification.description": "How the age rating is written in NFO files.",
  "settings.metadata.nfo-certification.keywords": "nfo, mpaa, rating, certification, fsk, age",
  "settings.metadata.nfo-certification.option.short": "Short (PG-13)",
  "settings.metadata.nfo-certification.option.short-mpaa": "Short, US as Rated (Rated PG-13)",
  "settings.metadata.nfo-certification.option.medium": "With country (US: PG-13)",
  "settings.metadata.nfo-certification.option.medium-full":
    "With country name (United States: PG-13)",
  "settings.metadata.nfo-certification.option.large": "All forms (US:PG-13 / US:Rated PG-13)",
  "settings.metadata.nfo-certification.option.large-full": "All forms, country names",
  "settings.metadata.nfo-certification.option.technical": "Technical (US_PG13)",
  "settings.metadata.nfo-certification.option.technical2": "Technical with dash (US-PG13)",
  "settings.metadata.nfo-create-outline.title": "Create the outline from the plot",
  "settings.metadata.nfo-create-outline.description":
    "Fill the NFO outline from the plot when a title has none.",
  "settings.metadata.nfo-create-outline.keywords": "nfo, outline, summary, plot",
  "settings.metadata.nfo-outline-first-sentence.title": "Outline is the first sentence",
  "settings.metadata.nfo-outline-first-sentence.description":
    "Use the first sentence of the plot for a created outline instead of the whole plot.",
  "settings.metadata.nfo-outline-first-sentence.keywords": "nfo, outline, sentence, plot",
  "settings.metadata.nfo-single-studio.title": "Write only the first studio",
  "settings.metadata.nfo-single-studio.description":
    "List one studio in NFO files instead of all of them.",
  "settings.metadata.nfo-single-studio.keywords": "nfo, studio, production company",
  "settings.metadata.nfo-date-added.title": "Write the date added",
  "settings.metadata.nfo-date-added.description": "Put a date-added entry in NFO files.",
  "settings.metadata.nfo-date-added.keywords": "nfo, dateadded, added, recently added",
  "settings.metadata.nfo-date-added-source.title": "Date added comes from",
  "settings.metadata.nfo-date-added-source.description": "Which date the date-added entry holds.",
  "settings.metadata.nfo-date-added-source.keywords":
    "nfo, dateadded, file creation, modified, release",
  "settings.metadata.nfo-date-added-source.option.added": "When added to CineVault",
  "settings.metadata.nfo-date-added-source.option.file-created": "File creation date",
  "settings.metadata.nfo-date-added-source.option.file-modified": "File modified date",
  "settings.metadata.nfo-date-added-source.option.release": "Release date",
  "settings.metadata.nfo-artwork-urls.title": "Write artwork web addresses",
  "settings.metadata.nfo-artwork-urls.description":
    "List the poster and fanart addresses and the cast pictures in NFO files.",
  "settings.metadata.nfo-artwork-urls.keywords": "nfo, thumb, fanart, poster, images, urls",
  "settings.metadata.nfo-trailer.title": "Write the trailer",
  "settings.metadata.nfo-trailer.description": "Put the trailer address in NFO files.",
  "settings.metadata.nfo-trailer.keywords": "nfo, trailer, youtube",
  "settings.metadata.nfo-fileinfo.title": "Write file details",
  "settings.metadata.nfo-fileinfo.description":
    "Put video, audio and subtitle details in NFO files.",
  "settings.metadata.nfo-fileinfo.keywords": "nfo, fileinfo, streams, codec, resolution",
  "settings.metadata.nfo-external-audio.title": "Include audio files next to the video",
  "settings.metadata.nfo-external-audio.description":
    "List audio streams of separate audio files in the file details.",
  "settings.metadata.nfo-external-audio.keywords": "nfo, fileinfo, external audio, streams",
  "settings.metadata.nfo-lockdata.title": "Write the lock-data flag",
  "settings.metadata.nfo-lockdata.description":
    "Tell Jellyfin and Emby not to change the metadata of these titles.",
  "settings.metadata.nfo-lockdata.keywords": "nfo, lockdata, lock, jellyfin, emby",
  "settings.metadata.nfo-clean.title": "Write clean NFO files",
  "settings.metadata.nfo-clean.description":
    "Write only what CineVault knows. Elements and watch history already in the file are not kept.",
  "settings.metadata.nfo-clean.keywords": "nfo, clean, unknown elements, custom fields, preserve",
  "settings.metadata.nfo-movie-names.title": "Movie NFO file names",
  "settings.metadata.nfo-movie-names.description":
    "The file name of a movie NFO. A folder with several movies always uses the video name.",
  "settings.metadata.nfo-movie-names.keywords": "nfo, naming, movie.nfo, file name",
  "settings.metadata.nfo-movie-names.option.video-base": "Video name (Movie (2010).nfo)",
  "settings.metadata.nfo-movie-names.option.movie": "movie.nfo",
  "settings.metadata.nfo-movie-names.option.both": "Both",
  "settings.metadata.nfo-disc-folder.title": "NFO inside disc folders",
  "settings.metadata.nfo-disc-folder.description":
    "Put the NFO of a disc movie in the disc folder, named after its main file.",
  "settings.metadata.nfo-disc-folder.keywords": "nfo, disc, bluray, dvd, video_ts, bdmv",
  "settings.metadata.nfo-set-names.title": "Collection NFO file names",
  "settings.metadata.nfo-set-names.description": "The file name of a collection NFO.",
  "settings.metadata.nfo-set-names.keywords": "nfo, collection, set, naming, collection.nfo",
  "settings.metadata.nfo-set-names.option.set-name": "Collection name (Name.nfo)",
  "settings.metadata.nfo-set-names.option.set": "set.nfo",
  "settings.metadata.nfo-set-names.option.collection": "collection.nfo",
  "settings.metadata.nfo-set-names.option.all": "All three",
  "settings.metadata.nfo-collection-folder.title": "Collection folder",
  "settings.metadata.nfo-collection-folder.description":
    "Where collection NFOs and the Kodi and Automator style collection artwork are written. Collection NFOs are skipped until it is set.",
  "settings.metadata.nfo-collection-folder.keywords":
    "nfo, collection, set, folder, artwork folder, data folder",
  "settings.metadata.nfo-collection-tmdb-id.title": "Add the TMDB ID to collection folders",
  "settings.metadata.nfo-collection-tmdb-id.description":
    "Append the TMDB collection ID to the name of the collection folder and files (Name-tmdb-123).",
  "settings.metadata.nfo-collection-tmdb-id.keywords":
    "nfo, collection, tmdb, folder name, artwork",
  "settings.metadata.collection-name-replacement.title": "Replace characters in collection names",
  "settings.metadata.collection-name-replacement.description":
    "What takes the place of each character a file name cannot hold (such as a colon) in collection folder and file names. Leave empty to remove them.",
  "settings.metadata.collection-name-replacement.keywords":
    "collection, set, name, characters, colon, replace, illegal, folder name",
  "settings.metadata.nfo-season-names.title": "Season NFO file names",
  "settings.metadata.nfo-season-names.description": "The file name of a season NFO.",
  "settings.metadata.nfo-season-names.keywords": "nfo, season, naming, season.nfo",
  "settings.metadata.nfo-season-names.option.season": "In the show folder (season01.nfo)",
  "settings.metadata.nfo-season-names.option.season-folder": "In the season folder (season.nfo)",
  "settings.metadata.nfo-season-names.option.both": "Both",
  "settings.metadata.nfo-all-actors.title": "Write all actors in the show NFO",
  "settings.metadata.nfo-all-actors.description":
    "Add the guest actors of the episodes to the show NFO.",
  "settings.metadata.nfo-all-actors.keywords": "nfo, actors, guest stars, cast, tv",
  "settings.metadata.nfo-episodeguide.title": "Episode guide",
  "settings.metadata.nfo-episodeguide.description":
    "How the episode guide entry of a show NFO is written.",
  "settings.metadata.nfo-episodeguide.keywords": "nfo, episodeguide, tvdb, tmdb, tv",
  "settings.metadata.nfo-episodeguide.option.json": "JSON with the show IDs",
  "settings.metadata.nfo-episodeguide.option.url": "Web address",
  "settings.metadata.nfo-episodeguide.option.off": "Do not write it",
  "settings.metadata.nfo-end-date.title": "Write the end date",
  "settings.metadata.nfo-end-date.description":
    "Add the date of the last episode to the NFO of an ended show (Jellyfin and Emby).",
  "settings.metadata.nfo-end-date.keywords": "nfo, enddate, ended, tv",
  "settings.artwork.auto-select.title": "Choose artwork automatically",
  "settings.artwork.auto-select.description":
    "Pick the best-rated image in your language for each type.",
  "settings.artwork.auto-select.keywords": "poster, fanart, images, automatic",
  "settings.metadata.artwork-names.title": "Artwork file names",
  "settings.metadata.artwork-names.description":
    "Prefixed names keep several movies in one folder apart.",
  "settings.metadata.artwork-names.keywords": "poster, fanart, naming, folder.jpg",
  "settings.artwork.extra-fanart-enabled.title": "Download extra fanart",
  "settings.artwork.extra-fanart-enabled.description":
    "Save more background images next to each title. Not written for folders that hold several movies.",
  "settings.artwork.extra-fanart-enabled.keywords": "extrafanart, backgrounds, images",
  "settings.artwork.extra-fanart.title": "Extra fanart per title",
  "settings.artwork.extra-fanart.description":
    "How many additional background images to download when extra fanart is on. The previous ones are replaced.",
  "settings.artwork.extra-fanart.keywords": "extrafanart, backgrounds, images, count",
  "settings.artwork.extra-thumbs.title": "Download extra thumbs",
  "settings.artwork.extra-thumbs.description":
    "Save additional thumbnails in an extrathumbs folder. Not written for folders that hold several movies.",
  "settings.artwork.extra-thumbs.keywords": "extrathumbs, images",
  "settings.artwork.extra-thumbs-count.title": "Extra thumbs per title",
  "settings.artwork.extra-thumbs-count.description":
    "How many additional thumbnails to download when extra thumbs are on.",
  "settings.artwork.extra-thumbs-count.keywords": "extrathumbs, images, count",
  "settings.artwork.extra-thumbs-resize.title": "Shrink extra thumbs",
  "settings.artwork.extra-thumbs-resize.description":
    "Scale extra thumbnails down to the width below. Smaller images are never enlarged.",
  "settings.artwork.extra-thumbs-resize.keywords": "extrathumbs, resize, scale, size",
  "settings.artwork.extra-thumbs-width.title": "Widest extra thumb",
  "settings.artwork.extra-thumbs-width.description":
    "The width in pixels extra thumbnails are scaled down to.",
  "settings.artwork.extra-thumbs-width.keywords": "extrathumbs, resize, width, pixels",
  "settings.artwork.collection-locations.title": "Collection artwork location",
  "settings.artwork.collection-locations.description":
    "Where posters and fanart of collections are saved. The two folder styles use the collection folder set under Movies.",
  "settings.artwork.collection-locations.keywords":
    "collection, set, artwork, poster, movieset, kodi, automator, folder",
  "settings.artwork.collection-locations.option.movie-folder":
    "In each movie folder (movieset-poster.jpg)",
  "settings.artwork.collection-locations.option.kodi":
    "Collection folder, Kodi style (Name/poster.jpg)",
  "settings.artwork.collection-locations.option.automator":
    "Collection folder, Artwork Automator style (Name-poster.jpg)",
  "settings.artwork.collection-locations.option.both": "Movie folders and Kodi style",
  "settings.artwork.actor-images.title": "Save actor photos",
  "settings.artwork.actor-images.description":
    "Save portraits in a .actors folder next to each title. Photos of people who left the cast are moved to the trash.",
  "settings.artwork.actor-images.keywords": "actors, cast, portraits, photos, .actors",
  "settings.player.kind.option.system": "System default",
  "settings.player.kind.option.mpv": "mpv",
  "settings.player.kind.option.vlc": "VLC",
  "settings.player.kind.option.mpc-hc": "MPC-HC / MPC-BE",
  "settings.player.kind.option.iina": "IINA",
  "settings.player.kind.option.custom": "Custom command",
  "settings.player.kind.title": "Video player",
  "settings.player.kind.description":
    "The player the Play button starts. The system default opens one file at a time; the other players queue every part of a stacked movie in order.",
  "settings.player.kind.keywords":
    "play, external player, media player, mpv, vlc, mpc-hc, mpc-be, iina, default player",
  "settings.player.path.title": "Player program",
  "settings.player.path.description":
    "Where the player is installed, when CineVault cannot find it by itself. Leave empty to look in the usual places.",
  "settings.player.path.keywords": "play, executable, exe, location, install folder, mpv, vlc",
  "settings.player.custom-command.title": "Custom player command",
  "settings.player.custom-command.description":
    "Used when the video player is Custom command. The first word is the program. {file}, {files} and {folder} stand for the video files of the title; without any of them the files are added at the end. Put quotes around words that contain spaces.",
  "settings.player.custom-command.keywords":
    "play, arguments, command line, placeholder, external player",
  "settings.advanced.size-units.title": "File size units",
  "settings.advanced.size-units.description":
    "Base 10 shows GB (1000-based), as Finder does. Base 2 shows GiB (1024-based), the numbers Windows Explorer shows, and is the default on Windows.",
  "settings.advanced.size-units.keywords": "base 10, base 2, gigabytes, gibibytes, decimal, binary",
  "settings.advanced.log-level.title": "Log detail",
  "settings.advanced.log-level.description":
    "More detail helps when reporting a problem. Secrets are never logged.",
  "settings.advanced.log-level.keywords": "logging, debug, diagnostics, verbose",
  "settings.torrents.source.torrentio.title": "Torrentio",
  "settings.torrents.source.torrentio.description": "Releases by IMDb ID for movies and episodes.",
  "settings.torrents.source.torrentio.keywords":
    "torrentio, stremio, torrent, magnet, releases, source",
  "settings.torrents.source.yts.title": "YTS",
  "settings.torrents.source.yts.description": "Movie releases by IMDb ID.",
  "settings.torrents.source.yts.keywords": "yts, torrent, magnet, movies, releases, source",
  "settings.torrents.source.eztv.title": "EZTV",
  "settings.torrents.source.eztv.description": "TV releases by IMDb ID.",
  "settings.torrents.source.eztv.keywords": "eztv, torrent, magnet, tv, shows, releases, source",
  "settings.torrents.source.apibay.title": "The Pirate Bay",
  "settings.torrents.source.apibay.description": "Text search on The Pirate Bay's JSON interface.",
  "settings.torrents.source.apibay.keywords":
    "pirate bay, apibay, tpb, torrent, magnet, releases, source",
  "settings.torrents.source.knaben.title": "Knaben",
  "settings.torrents.source.knaben.description": "Text search over many public trackers at once.",
  "settings.torrents.source.knaben.keywords":
    "knaben, torrent, magnet, aggregator, releases, source",
  "settings.torrents.source.nyaa.title": "Nyaa",
  "settings.torrents.source.nyaa.description":
    "Anime releases. Only asked when the title is anime.",
  "settings.torrents.source.nyaa.keywords": "nyaa, anime, torrent, magnet, releases, source",
  "settings.torrents.indexer-service.enabled.title": "Indexer service",
  "settings.torrents.indexer-service.enabled.description":
    "Download and run the indexer service while CineVault is open, for many more sites. It starts with the app and stops when the app closes.",
  "settings.torrents.indexer-service.enabled.keywords":
    "indexer, jackett, service, torrent, sites, download, background",
  "settings.torrents.torznab.url.title": "Your own Torznab address",
  "settings.torrents.torznab.url.description":
    "The feed address of your own Torznab instance, searched as one more source. Add the key and test it in the card above.",
  "settings.torrents.torznab.url.keywords":
    "torznab, jackett, prowlarr, indexer, feed, url, torrent",
  "settings.torrents.indexer-service.solver-url.title": "Challenge solver address",
  "settings.torrents.indexer-service.solver-url.description":
    "The address of a challenge solver the indexer service may use for sites that need a browser check. Leave empty for none.",
  "settings.torrents.indexer-service.solver-url.keywords":
    "solver, flaresolverr, challenge, cloudflare, indexer, proxy",
  "settings.torrents.url.torrentio.title": "Torrentio address",
  "settings.torrents.url.torrentio.description":
    "The web address of this source, when the site has moved. Reset restores https://torrentio.strem.fun.",
  "settings.torrents.url.torrentio.keywords": "torrentio, address, url, mirror, domain, torrent",
  "settings.torrents.url.yts.title": "YTS address",
  "settings.torrents.url.yts.description":
    "The web address of this source, when the site has moved. Reset restores https://movies-api.accel.li.",
  "settings.torrents.url.yts.keywords": "yts, address, url, mirror, domain, torrent",
  "settings.torrents.url.eztv.title": "EZTV address",
  "settings.torrents.url.eztv.description":
    "The web address of this source, when the site has moved. Reset restores https://eztvx.to.",
  "settings.torrents.url.eztv.keywords": "eztv, address, url, mirror, domain, torrent",
  "settings.torrents.url.apibay.title": "The Pirate Bay address",
  "settings.torrents.url.apibay.description":
    "The web address of this source, when the site has moved. Reset restores https://apibay.org.",
  "settings.torrents.url.apibay.keywords": "apibay, address, url, mirror, domain, torrent",
  "settings.torrents.url.knaben.title": "Knaben address",
  "settings.torrents.url.knaben.description":
    "The web address of this source, when the site has moved. Reset restores https://api.knaben.org.",
  "settings.torrents.url.knaben.keywords": "knaben, address, url, mirror, domain, torrent",
  "settings.torrents.url.nyaa.title": "Nyaa address",
  "settings.torrents.url.nyaa.description":
    "The web address of this source, when the site has moved. Reset restores https://nyaa.si.",
  "settings.torrents.url.nyaa.keywords": "nyaa, address, url, mirror, domain, torrent",
  "updates.source": "Updates",
  "updates.title": "CineVault {version} is available",
  "updates.notNow": "Not now",
  "updates.install": "Update now",
  "updates.cancel": "Cancel",
  "updates.installing.title": "Installing CineVault {version}…",
  "updates.jobs.title": "A job is running.",
  "updates.jobs.body": "Updating restarts CineVault, which stops the running work.",
  "updates.jobs.wait": "Update after it finishes",
  "updates.jobs.stop": "Update now and stop it",
  "updates.waiting.title": "CineVault {version} installs when the running jobs finish.",
  "updates.started.body": "Follow its steps to finish updating CineVault.",
  "updates.upToDate": "CineVault is up to date",
  "updates.checkFailed": "Could not check for updates",
  "updates.checkFailedWith": "Could not check for updates. {reason}",
  "updates.check.noChannelHere": "No update channel is set for this build.",
  "updates.check.noKeyHere": "This build has no update signing key.",
  "updates.show": "Show",
  "updates.installFailed": "The update was not installed",
  "updates.desktopOnly": "Updates are available in the desktop app.",

  // The Settings page of the desktop redesign (features/settings/SettingsPage.tsx)
  "settingsPage.region": "Settings",
  "settingsPage.index": "Settings sections",
  "settingsPage.providers.title": "Providers",
  "settingsPage.providers.hint": "Keys are stored in the system credential store, never in files.",
  "settingsPage.providers.tmdb": "TMDB key",
  "settingsPage.providers.tmdb.hint": "Needed for Scrape.",
  "settingsPage.providers.omdb": "OMDb key",
  "settingsPage.providers.omdb.hint": "Adds IMDb, Rotten Tomatoes and Metacritic ratings.",
  "settingsPage.providers.mdblist": "MDBList key",
  "settingsPage.providers.mdblist.hint": "Adds Letterboxd and more ratings.",
  "settingsPage.providers.fanart": "Fanart.tv key",
  "settingsPage.providers.fanart.hint": "Adds more artwork.",
  "settingsPage.providers.tvdb": "TheTVDB key",
  "settingsPage.providers.tvdb.hint": "Adds TV artwork and episode data.",
  "settingsPage.providers.getKey": "Get a free key",
  "settingsPage.providers.language": "Metadata language",
  "settingsPage.providers.language.hint": "Titles, plots and artwork text",
  "settingsPage.providers.country": "Ratings country",
  "settingsPage.providers.countryOption": "Ratings: {country}",
  "settingsPage.keys.paste": "Paste your key",
  "settingsPage.keys.unavailable": "Keys can be set in the desktop app",
  "settingsPage.keys.works": "Works",
  "settingsPage.keys.rejectedTag": "Rejected",
  "settingsPage.keys.rejected": "{name} did not accept this key. Check that it is copied whole.",
  "settingsPage.keys.show": "Show",
  "settingsPage.keys.hide": "Hide",
  "settingsPage.keys.remove": "Remove",
  "settingsPage.keys.test": "Test",
  "settingsPage.keys.testing": "Testing",
  "settingsPage.keys.testFor": "Test the {name} key",
  "settingsPage.naming.title": "Naming",
  "settingsPage.naming.example": "Example:",
  "settingsPage.naming.template": "{name} template",
  "settingsPage.naming.tokens": "Tokens for {name}",
  "settingsPage.naming.movieFolder": "Movie folder",
  "settingsPage.naming.movieFolder.hint": "Used by Rename",
  "settingsPage.naming.movieFile": "Movie file",
  "settingsPage.naming.movieFile.hint": "Extras and subtitles follow the main file",
  "settingsPage.naming.episode": "TV episode",
  "settingsPage.naming.colon": "Colon in titles",
  "settingsPage.naming.colon.remove": "Remove",
  "settingsPage.naming.colon.dash": "Use “ - ”",
  "settingsPage.naming.colon.lookalike": "Use “꞉”",
  "settingsPage.folders.title": "Library folders",
  "settingsPage.folders.movies": "Movies",
  "settingsPage.folders.tv": "TV shows",
  "settingsPage.folders.holds": "{path} holds",
  "settingsPage.folders.titles": "{count, plural, one {# title} other {# titles}}",
  "settingsPage.folders.shows": "{count, plural, one {# show} other {# shows}}",
  "settingsPage.folders.watched": "watched for changes",
  "settingsPage.folders.offline.titles":
    "offline · {count, plural, one {# title} other {# titles}} kept",
  "settingsPage.folders.offline.shows":
    "offline · {count, plural, one {# show} other {# shows}} kept",
  "settingsPage.folders.remove": "Remove",
  "settingsPage.folders.removeFor": "Remove {path}",
  "settingsPage.folders.removed":
    "{count, plural, =0 {Removed {path}} one {Removed {path} · # title stays, offline} other {Removed {path} · # titles stay, offline}}",
  "settingsPage.folders.add": "Add folder…",
  "settingsPage.folders.drop": "or drop folders here",
  "settingsPage.folders.added": "{count, plural, one {Added # folder} other {Added # folders}}",
  "settingsPage.torrents.title": "Torrent sources",
  "settingsPage.torrents.sources.hint": "The sites Torrent search asks",
  "settingsPage.appearance.title": "Appearance",
  "settingsPage.deleted.title": "Deleted files",
  "settingsPage.deleted.help.bin":
    "Undo brings them back. A drive without a Recycle Bin uses its .deletedByCineVault folder.",
  "settingsPage.deleted.help.folder":
    "Always the .deletedByCineVault folder on the file's own drive; the Recycle Bin is never used.",
  "settingsPage.deleted.help.custom":
    "A folder you choose, for files on its drive; files on other drives go to their own drive's trash folder.",
  "settingsPage.deleted.help.erase":
    "Deleted and replaced files are erased at once. Undo can't bring them back.",
  "settingsPage.deleted.noFolder":
    "Choose a folder; until then files go to each drive's trash folder.",
  "settingsPage.deleted.choose": "Choose folder…",
  "settingsPage.deleted.pickTitle": "Choose the custom trash folder",
  "settingsPage.deleted.confirmTitle": "Erase deleted files at once?",
  "settingsPage.deleted.confirm": "Delete permanently",
  "settingsPage.appearance.theme": "Theme",
  "settingsPage.appearance.light": "Light",
  "settingsPage.appearance.dark": "Dark",
  "settingsPage.appearance.match": "Match Windows",
  "settingsPage.appearance.matchMac": "Match macOS",
  "settingsPage.appearance.size": "Size",
  "settingsPage.appearance.size.hint": "Ctrl + and Ctrl − change it, Ctrl 0 resets.",
  "settingsPage.appearance.size.hintMac": "⌘ + and ⌘ − change it, ⌘ 0 resets.",
  "settingsPage.appearance.sizeOption": "{percent}%",
  "settingsPage.updates.title": "Updates",
  "settingsPage.updates.version": "CineVault {version}",
  "settingsPage.updates.checkNow": "Check now",
  "settingsPage.updates.checking": "Checking for updates…",
  "settingsPage.updates.notChecked": "Not checked yet",
  "settingsPage.updates.upToDate": "Up to date",
  "settingsPage.updates.today": "checked today at {time}",
  "settingsPage.updates.on": "checked {date} at {time}",
  "settingsPage.updates.downloading": "Downloading CineVault {version} · {percent}%",
  "settingsPage.updates.installOnClose": "Install updates when I close the app",
} as const;
