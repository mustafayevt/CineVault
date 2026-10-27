/** The sidebar's destinations, in order. */
export type Destination =
  "library" | "scrape" | "rename" | "drives" | "torrents" | "history" | "settings";

/** Which destination a page belongs to; title and picker pages belong to the Library. */
export function destinationOf(pathname: string): Destination | null {
  const under = (base: string) => pathname === base || pathname.startsWith(`${base}/`);
  if (under("/library") || under("/movies") || under("/tv") || under("/collections"))
    return "library";
  if (under("/artwork")) return "library";
  if (under("/scrape")) return "scrape";
  if (under("/rename")) return "rename";
  if (under("/drives")) return "drives";
  if (under("/torrents")) return "torrents";
  if (under("/history")) return "history";
  if (under("/settings") || under("/release-notes")) return "settings";
  return null;
}
