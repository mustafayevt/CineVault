/** English strings of the common area. Merged with the other areas in `../../catalog.ts`. */
export const common = {
  // Common words
  "common.close": "Close",
  "common.dismiss": "Dismiss",
  "common.undo": "Undo",
  "common.unknownSize": "Unknown size",

  // Status vocabulary (StatusTag): a glyph plus one of these words
  "status.done": "Done",
  "status.waiting": "Waiting",
  "status.missing": "Missing",
  "status.needsKey": "Needs a key",
  "status.inProgress": "In progress",
  "status.off": "Off",

  // Shared components
  "components.removeFilter": "Remove filter {field} {value}",
  "components.editFilter": "Edit filter {chip}",
  "components.editFilter.hint": "Click to edit, Alt+click to negate",
  "components.negated": "not",

  // File sizes: unit symbols. Base 10 uses powers of 1000, base 2 powers of 1024.
  "size.value": "{value} {unit}",
  "size.base10.0": "B",
  "size.base10.1": "kB",
  "size.base10.2": "MB",
  "size.base10.3": "GB",
  "size.base10.4": "TB",
  "size.base10.5": "PB",
  "size.base2.0": "B",
  "size.base2.1": "KiB",
  "size.base2.2": "MiB",
  "size.base2.3": "GiB",
  "size.base2.4": "TiB",
  "size.base2.5": "PiB",

  // Named post-processing scripts
  "path.copyFailed": "The path could not be copied",
  "value.copied": "Copied {value}",
  "stage.new": "New",
  "stage.scraped": "Scraped",
  "stage.renamed": "Renamed",
  "stage.flag.needsReview": "Needs review",
  "stage.flag.notMatched": "Not matched",
} as const;
