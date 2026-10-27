/** English strings of the first-run setup. Merged with the other areas in `../../catalog.ts`. */
export const onboarding = {
  // Frame
  "setup.region": "First run setup",
  "setup.steps": "Setup steps",
  "setup.step.folders": "Folders",
  "setup.step.tmdb": "TMDB key",
  "setup.step.ratings": "Ratings keys",
  "setup.step.naming": "Naming",
  "setup.step.done": "done",
  "setup.skipSetup": "Skip setup",
  "setup.skipStep": "Skip this step",
  "setup.continue": "Continue",
  "setup.building": "Setting up…",
  "setup.laterNote": "Every step can be changed later in Settings.",
  "setup.buildFailed": "This folder could not be added. {reason}",
  "setup.tools.failed": "{name} could not be downloaded.",

  // Step 1: folders
  "setup.folders.title": "Where are your movies and shows?",
  "setup.folders.intro":
    "Add each folder and say what it holds. CineVault scans them now and whenever they change. Nothing is renamed or moved until you ask.",
  "setup.folders.list": "Folders",
  "setup.folders.looking": "Looking inside…",
  "setup.folders.found": "{count, plural, one {# video found} other {# videos found}}",
  "setup.folders.foundMore":
    "{count, plural, one {# video found so far} other {# videos found so far}}",
  "setup.folders.looksTv": "looks like TV shows",
  "setup.folders.both": "holds movies and shows, choose one",
  "setup.folders.inside": "Inside {other}, which is in the list already",
  "setup.folders.holds": "{path} holds",
  "setup.folders.movies": "Movies",
  "setup.folders.tv": "TV shows",
  "setup.folders.remove": "Remove {path}",
  "setup.folders.add": "Add folder…",
  "setup.folders.drop": "or drop folders here",
  "setup.folders.unreadable": "{path} could not be read.",
  "setup.folders.stillLooking": "CineVault is still looking inside a folder.",
  "setup.folders.overlap": "A folder lies inside another one of the same type. Remove one of them.",
  "setup.folders.needOne": "Add a folder first.",
  "setup.folders.needOneFix": "Add a folder, or skip this step.",

  // Step 2: TMDB
  "setup.tmdb.title": "Add your TMDB key",
  "setup.tmdb.intro":
    "CineVault finds titles, posters and plots through TMDB. The key is free and stays on this computer.",
  "setup.tmdb.step1": "Sign in to a free TMDB account.",
  "setup.tmdb.step2":
    "Open its API settings and request a key (the Developer type is the right one).",
  "setup.tmdb.step3": "Paste the API Key above and press Test.",
  "setup.tmdb.needsKey": "Test your TMDB key first.",
  "setup.tmdb.needsKeyFix": "Paste the key and press Test, or skip this step.",

  // Step 3: optional keys (ratings and artwork)
  "setup.ratings.title": "Add your optional keys",
  "setup.ratings.intro":
    "All are optional. Each key adds ratings or artwork to your titles, and any can be added later in Settings.",
  "setup.ratings.optional": "Optional",
  "setup.ratings.omdb": "Adds IMDb, Rotten Tomatoes and Metacritic ratings",
  "setup.ratings.mdblist": "Adds Letterboxd and more ratings",
  "setup.ratings.fanart": "Adds more artwork",
  "setup.ratings.tvdb": "Adds TV artwork and episode data",

  // Step 4: naming
  "setup.naming.title": "How should folders and files be named?",
  "setup.naming.intro":
    "Pick a style. CineVault uses it when you rename on the Rename page; the templates can be edited in Settings.",
  "setup.naming.styles": "Naming styles",
  "setup.naming.style.clean": "Title and year, short names every media centre reads.",
  "setup.naming.style.detailed": "Adds the resolution, source, HDR and audio tags.",
  "setup.naming.style.media-server-safe": "Adds the TMDB id so media servers match the title.",
  "setup.naming.style.custom": "Your own templates.",
  "setup.naming.example": "Example",
  "setup.naming.sample": "Built-in sample",
  "setup.naming.yours": "Your file",
  "setup.naming.failed": "The example could not be made.",
  "setup.naming.loading": "Making the example…",
  "setup.naming.now": "Now",
  "setup.naming.folder": "Folder",
  "setup.naming.file": "File",
  "setup.naming.nothing": "Nothing is renamed until you ask on the Rename page.",

  // Preview in a plain browser
  "onboarding.desktopOnly": "Folders can be looked into in the desktop app.",
} as const;
