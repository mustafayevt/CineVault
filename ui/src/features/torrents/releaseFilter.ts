/**
 * The "Filter results" field of a title's releases (spec `torrent-search` "Filter the results by
 * text"): every word must be in the release name, a word starting with `-` must not. Case and
 * punctuation do not count, so `web-dl` matches `WEB.DL` and `5.1` matches `DD5.1`.
 */
export interface ReleaseFilter {
  readonly wanted: readonly string[];
  readonly unwanted: readonly string[];
}

/** Lower case, letters and digits only. */
function bare(text: string): string {
  return text.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");
}

/** The words of the typed text; words that are only punctuation are left out. */
export function parseReleaseFilter(text: string): ReleaseFilter {
  const wanted: string[] = [];
  const unwanted: string[] = [];
  for (const word of text.split(/\s+/)) {
    const excluded = word.startsWith("-");
    const core = bare(excluded ? word.slice(1) : word);
    if (core === "") continue;
    (excluded ? unwanted : wanted).push(core);
  }
  return { wanted, unwanted };
}

/** The filter asks for nothing: every release passes. */
export function isEmptyFilter(filter: ReleaseFilter): boolean {
  return filter.wanted.length === 0 && filter.unwanted.length === 0;
}

/** The release name holds every wanted word and no unwanted one. */
export function matchesFilter(name: string, filter: ReleaseFilter): boolean {
  const plain = bare(name);
  return (
    filter.wanted.every((word) => plain.includes(word)) &&
    !filter.unwanted.some((word) => plain.includes(word))
  );
}
