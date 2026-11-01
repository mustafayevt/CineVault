/** English strings of the copies area. Merged with the other areas in `../../catalog.ts`. */
export const copies = {
  // The Library: the filter chip, the poster badge, the table, the right-click menu
  "copies.chip": "Copies",
  "copies.badge": "{count} copies",
  "copies.badge.tip": "You have {count} copies of this movie",
  "copies.table.count": "{count} copies",
  "copies.act.compare": "Compare {count} copies",

  // The Movie page line
  "copies.movie.line": "You have {count} copies",
  "copies.movie.compare": "Compare",

  // The Compare copies page
  "copies.page.region": "Compare copies of {title}",
  "copies.page.title": "Compare copies · {title}",
  "copies.page.cancel": "Cancel",
  "copies.page.sub": "{count} copies · {size} in all",
  "copies.page.sub.same": "{count} copies · {size} in all · same picture, sound and size",
  "copies.page.same.two": "These two copies look the same.",
  "copies.page.same.many": "These {count} copies look the same.",
  "copies.page.lengths": "Lengths differ by {minutes} min — these may be different films.",
  "copies.lengthDiffers": "Lengths differ",
  "copies.page.same.frees": "Keeping one frees {size}, and nothing about the movie changes.",
  "copies.page.sameFile.before":
    "These may be the same file reached through two library folders. CineVault won't delete either from here; check the folders under ",
  "copies.page.sameFile.link": "Settings › Library folders",
  "copies.page.sameFile.after": ".",
  "copies.page.intro.before":
    "These are the same movie. Keep the one you want; the others are deleted the way ",
  "copies.page.intro.link": "Settings › Deleted files",
  "copies.page.intro.after": " says.",
  "copies.dest.bin":
    "Right now that is the {bin}, so Undo brings {count, plural, one {the other copy} other {the other # copies}} back.",
  "copies.dest.folder":
    "Right now that is a trash folder on each drive, so Undo brings {count, plural, one {the other copy} other {the other # copies}} back.",
  "copies.dest.custom":
    "Right now that is your trash folder {folder}, so Undo brings {count, plural, one {the other copy} other {the other # copies}} back.",
  "copies.dest.erase":
    "Right now that is Delete permanently: you are asked first, and {count, plural, one {the other copy cannot} other {the other # copies cannot}} be brought back.",
  "copies.none.title": "This movie has no other copies",
  "copies.none.body": "The other copy was deleted, moved or is not reachable.",
  "copies.none.back": "Back to the movie",
  "copies.loading": "Reading the copies",
  "copies.error": "The copies could not be read",

  // The rows
  "copies.row.resolution": "Resolution",
  "copies.row.resolution.hint": "How sharp the picture is",
  "copies.row.hdr": "HDR",
  "copies.row.hdr.hint": "Richer light and colour on HDR screens",
  "copies.row.audio": "Audio",
  "copies.row.audio.hint": "Sound format and channels",
  "copies.row.source": "Source",
  "copies.row.source.hint": "Where the copy came from",
  "copies.row.bitrate": "Video bitrate",
  "copies.row.bitrate.hint": "Higher keeps more detail",
  "copies.row.length": "Length",
  "copies.row.length.hint": "Running time",
  "copies.row.size": "Size",
  "copies.row.size.hint": "Space on the drive",
  "copies.row.folder": "Folder",
  "copies.row.folder.hint": "Where it is",
  "copies.res.2160p": "4K",
  "copies.res.1440p": "QHD",
  "copies.res.1080p": "Full HD",
  "copies.res.720p": "HD",
  "copies.value.sdr": "SDR",
  "copies.value.unread": "Not read yet",
  "copies.value.unknown": "Unknown",
  "copies.value.none": "None",
  "copies.value.bitrate": "{value} Mb/s",
  "copies.value.smaller": "{size} smaller",

  // The cards
  "copies.card.aria": "Copy {index} of {count}: {title} on {drive}",
  "copies.card.network": "On the network",
  "copies.card.local": "On this computer",
  "copies.best": "Best quality",
  "copies.better": "Better",
  "copies.keep": "Keep this one",
  "copies.keep.aria": "Keep this one: {title}, {summary}",
  "copies.keep.refused": "Nothing was deleted",

  // Keep both, Ask again
  "copies.keepBoth": "Keep both",
  "copies.keepAll": "Keep all",
  "copies.keepBoth.note": "Keeps every file. These stop showing under Copies.",
  "copies.kept": "You chose to keep these copies",
  "copies.askAgain": "Ask again",
  "copies.toast.kept.two": "Kept both copies of {title}",
  "copies.toast.kept.many": "Kept all {count} copies of {title}",
  "copies.toast.askedAgain": "The copies of {title} show under Copies again",
  "copies.failed": "The decision could not be saved",

  // After Keep this one
  "copies.dropLabel": "Rename to drop the '- {label}' label?",

  // The Rename preview
  "copies.rename.hint.two": "Added so your two copies keep different names",
  "copies.rename.hint.many": "Added so your {count} copies keep different names",
  "copies.rename.nth": "Copy {n} of {count}",
} as const;
