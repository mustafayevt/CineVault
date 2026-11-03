import { catalogTitle } from "./syntheticCatalog";
import type { CopyFacts } from "./syntheticCatalog";
import type {
  WishAdd,
  WishCheck,
  WishCriteria,
  WishEntry,
  WishHeld,
  WishMatch,
  WishMatchState,
  WishScope,
} from "./torrentBackend";
import { TorrentRequestError } from "./torrentBackend";

/**
 * The synthetic Wishlist (`bun run dev`, `bun run shots` and the tests): an in-memory list that
 * starts with a few entries (`demo`) or none (`empty`). "Check now" finds one new release for
 * Heat the first time; a due-only check finds nothing, as every demo entry was checked lately.
 * What the library holds comes from the synthetic catalog.
 */

const MINUTE_MS = 60_000;
const GB = 1_000_000_000;

export type SyntheticWishlistStart = "demo" | "empty";

export interface SyntheticWishlistCalls {
  /** `check:<dueOnly>`, `add:<kind>:<tmdbId>`, `update:<id>`, `remove:<id>`, `keep:<id>`, `match:<id>:<key>:<state>`, `seen`. */
  wishlist: string[];
}

function match(
  key: string,
  name: string,
  sizeGb: number,
  seeders: number,
  quality: Pick<WishMatch, "resolution" | "source" | "codec" | "hdr" | "audio">,
  state: "new" | "seen" | "sent",
  found: number,
): WishMatch {
  return {
    key,
    name,
    size: sizeGb * GB,
    seeders,
    magnet: `magnet:?xt=urn:btih:${key}`,
    torrentUrl: null,
    sources: ["torrentio", "yts"],
    found,
    state,
    ...quality,
  };
}

const ANY: WishCriteria = {
  minResolution: null,
  sources: [],
  hdr: false,
  minSize: null,
  maxSize: null,
  minSeeders: null,
};

const RESOLUTION_RANK: Record<string, number> = { "2160p": 4, "1080p": 3, "720p": 2, "480p": 1 };

function heldOf(copy: CopyFacts): WishHeld {
  return { resolution: copy.resolution, source: copy.source, hdr: copy.hdr };
}

/** What the synthetic library holds of a title and scope, as the backend would say it. */
export function syntheticHeld(kind: string, tmdbId: number, scope: WishScope): WishHeld | null {
  const title = catalogTitle(kind === "show" ? "show" : "movie", tmdbId);
  if (title === undefined || title.inLibrary === null) return null;
  if (kind === "movie") {
    const copies = [title.copy, ...(title.moreCopies ?? [])].filter(
      (copy): copy is CopyFacts => copy !== undefined,
    );
    copies.sort(
      (a, b) => (RESOLUTION_RANK[b.resolution] ?? 0) - (RESOLUTION_RANK[a.resolution] ?? 0),
    );
    const best = copies[0];
    return best === undefined ? null : heldOf(best);
  }
  const held = title.held ?? [];
  const missing = title.missing ?? [];
  const unknown: WishHeld = { resolution: "unknown", source: null, hdr: null };
  if (scope.kind === "season") {
    const some = held.some((episode) => episode.season === scope.season);
    const gap = missing.some((episode) => episode.season === scope.season);
    return some && !gap ? unknown : null;
  }
  if (scope.kind === "episode") {
    return held.some((e) => e.season === scope.season && e.episode === scope.episode)
      ? unknown
      : null;
  }
  return null;
}

function demoEntries(now: number): WishEntry[] {
  const base = {
    posterKey: null,
    note: null,
    noteDetail: null,
    done: null,
    held: null,
    target: null,
    inLibrary: null,
    matches: [],
  };
  return [
    {
      ...base,
      id: 1,
      kind: "movie",
      tmdbId: 2,
      title: "Dune",
      year: 2021,
      scope: { kind: "movie", season: null, episode: null },
      criteria: {
        ...ANY,
        minResolution: "2160p",
        sources: ["Remux"],
        hdr: true,
        maxSize: 80 * 1024 ** 3,
      },
      added: now - 6 * 24 * 60 * MINUTE_MS,
      checked: now - 25 * MINUTE_MS,
      matches: [
        match(
          "d1a2",
          "Dune.2021.2160p.UHD.BluRay.REMUX.DV.HDR.HEVC.TrueHD.Atmos.7.1-FRAME",
          72,
          85,
          { resolution: "2160p", source: "Remux", codec: "HEVC", hdr: "DV", audio: "Atmos 7.1" },
          "seen",
          now - 2 * 24 * 60 * MINUTE_MS,
        ),
        match(
          "d3b4",
          "Dune.2021.2160p.BluRay.REMUX.HDR10.HEVC.DTS-HD.MA.7.1-SIC",
          61,
          23,
          {
            resolution: "2160p",
            source: "Remux",
            codec: "HEVC",
            hdr: "HDR10",
            audio: "DTS-HD 7.1",
          },
          "sent",
          now - 3 * 24 * 60 * MINUTE_MS,
        ),
      ],
    },
    {
      ...base,
      id: 2,
      kind: "show",
      tmdbId: 1,
      title: "Severance",
      year: 2022,
      scope: { kind: "next", season: null, episode: null },
      criteria: { ...ANY, minResolution: "1080p", sources: ["WEB-DL"] },
      added: now - 20 * 24 * 60 * MINUTE_MS,
      checked: now - 25 * MINUTE_MS,
      target: { season: 2, episode: 4 },
      inLibrary: { itemId: "1", name: "Severance (2022)" },
      matches: [
        match(
          "s2e4",
          "Severance.S02E04.Woes.Hollow.1080p.ATVP.WEB-DL.DDP5.1.Atmos.H.264-FLUX",
          4,
          310,
          { resolution: "1080p", source: "WEB-DL", codec: "AVC", hdr: null, audio: "DD+ 5.1" },
          "seen",
          now - 4 * 60 * MINUTE_MS,
        ),
      ],
    },
    {
      ...base,
      id: 3,
      kind: "movie",
      tmdbId: 3,
      title: "Heat",
      year: 1995,
      scope: { kind: "movie", season: null, episode: null },
      criteria: { ...ANY, minResolution: "2160p", hdr: true, minSize: 20 * GB, minSeeders: 10 },
      added: now - 2 * 24 * 60 * MINUTE_MS,
      checked: now - 25 * MINUTE_MS,
      held: syntheticHeld("movie", 3, { kind: "movie", season: null, episode: null }),
      inLibrary: { itemId: "3", name: "Heat (1995)" },
    },
    {
      ...base,
      id: 4,
      kind: "show",
      tmdbId: 4,
      title: "Shōgun",
      year: 2024,
      scope: { kind: "next", season: null, episode: null },
      criteria: { ...ANY, minResolution: "2160p" },
      added: now - 9 * 24 * 60 * MINUTE_MS,
      checked: now - 25 * MINUTE_MS,
      note: "airs",
      noteDetail: "2026-11-18",
      target: { season: 2, episode: 1 },
    },
    {
      ...base,
      id: 5,
      kind: "movie",
      tmdbId: 7,
      title: "Oppenheimer",
      year: 2023,
      scope: { kind: "movie", season: null, episode: null },
      criteria: { ...ANY, minResolution: "2160p" },
      added: now - 30 * 24 * 60 * MINUTE_MS,
      checked: now - 3 * 24 * 60 * MINUTE_MS,
      done: now - 2 * 24 * 60 * MINUTE_MS,
      inLibrary: { itemId: "7", name: "Oppenheimer (2023)" },
    },
  ];
}

const HEAT_FIND = match(
  "h5c6",
  "Heat.1995.2160p.UHD.BluRay.REMUX.HDR.HEVC.DTS-HD.MA.5.1-GRP",
  64,
  41,
  { resolution: "2160p", source: "Remux", codec: "HEVC", hdr: "HDR10", audio: "DTS-HD 5.1" },
  "new",
  0,
);

function sameScope(a: WishScope, b: WishScope): boolean {
  return a.kind === b.kind && a.season === b.season && a.episode === b.episode;
}

export function createSyntheticWishlist(start: SyntheticWishlistStart = "demo") {
  const calls: SyntheticWishlistCalls = { wishlist: [] };
  let entries: WishEntry[] = start === "demo" ? demoEntries(Date.now()) : [];
  const dismissed = new Set<string>();
  let nextId = entries.length + 1;
  let heatFound = false;
  const copy = (): WishEntry[] =>
    entries.map((entry) => ({
      ...entry,
      matches: entry.matches
        .filter((found) => !dismissed.has(`${entry.id}:${found.key}`))
        .map((found) => ({ ...found })),
    }));
  const find = (id: number): WishEntry => {
    const entry = entries.find((row) => row.id === id);
    if (entry === undefined)
      throw new TorrentRequestError("not_found", "That title was not found.");
    return entry;
  };
  const replace = (id: number, change: (entry: WishEntry) => WishEntry) => {
    entries = entries.map((row) => (row.id === id ? change(row) : row));
  };

  return {
    wishlistCalls: calls,
    wishlist: (): Promise<WishEntry[]> => Promise.resolve(copy()),
    wishlistAdd: (request: WishAdd): Promise<number> => {
      calls.wishlist.push(`add:${request.kind}:${request.tmdbId}`);
      const title = catalogTitle(request.kind === "show" ? "show" : "movie", request.tmdbId);
      const existing = entries.find(
        (row) =>
          row.kind === request.kind &&
          row.tmdbId === request.tmdbId &&
          sameScope(row.scope, request.scope),
      );
      if (existing !== undefined) {
        replace(existing.id, (row) => ({
          ...row,
          criteria: request.criteria,
          held: syntheticHeld(row.kind, row.tmdbId, row.scope),
          done: null,
          checked: null,
          matches: row.matches.filter((found) => found.state === "sent"),
        }));
        return Promise.resolve(existing.id);
      }
      const id = nextId++;
      entries = [
        {
          id,
          kind: request.kind,
          tmdbId: request.tmdbId,
          title: title?.title ?? `Title ${request.tmdbId}`,
          year: title?.year ?? null,
          posterKey: null,
          scope: request.scope,
          criteria: request.criteria,
          added: Date.now(),
          checked: null,
          note: null,
          noteDetail: null,
          done: null,
          held: syntheticHeld(request.kind, request.tmdbId, request.scope),
          target: null,
          inLibrary:
            title === undefined || title.inLibrary === null
              ? null
              : { itemId: String(title.tmdbId), name: title.inLibrary },
          matches: [],
        },
        ...entries,
      ];
      return Promise.resolve(id);
    },
    wishlistUpdate: (id: number, scope: WishScope, criteria: WishCriteria): Promise<void> => {
      calls.wishlist.push(`update:${id}`);
      const entry = find(id);
      const taken = entries.some(
        (row) =>
          row.id !== id &&
          row.kind === entry.kind &&
          row.tmdbId === entry.tmdbId &&
          sameScope(row.scope, scope),
      );
      if (taken) {
        return Promise.reject(
          new TorrentRequestError("invalid", "That is already on your Wishlist."),
        );
      }
      replace(id, (row) => ({
        ...row,
        scope,
        criteria,
        held: syntheticHeld(row.kind, row.tmdbId, scope),
        done: null,
        checked: null,
        matches: row.matches.filter((found) => found.state === "sent"),
      }));
      return Promise.resolve();
    },
    wishlistHeld: (kind: string, tmdbId: number, scope: WishScope): Promise<WishHeld | null> =>
      Promise.resolve(syntheticHeld(kind, tmdbId, scope)),
    wishlistKeepLooking: (id: number): Promise<void> => {
      calls.wishlist.push(`keep:${id}`);
      find(id);
      replace(id, (row) => ({
        ...row,
        held: syntheticHeld(row.kind, row.tmdbId, row.scope),
        done: null,
        checked: null,
        matches: row.matches.filter((found) => found.state === "sent"),
      }));
      return Promise.resolve();
    },
    wishlistRemove: (id: number): Promise<void> => {
      calls.wishlist.push(`remove:${id}`);
      entries = entries.filter((row) => row.id !== id);
      return Promise.resolve();
    },
    wishlistCheck: (dueOnly: boolean): Promise<WishCheck> => {
      calls.wishlist.push(`check:${dueOnly}`);
      const now = Date.now();
      let checked = 0;
      let newMatches = 0;
      entries = entries.map((row) => {
        const due = row.checked === null || now - row.checked >= 6 * 60 * MINUTE_MS;
        if (row.done !== null || (dueOnly && !due)) return row;
        checked += 1;
        if (row.kind === "movie" && row.tmdbId === 3 && !heatFound) {
          heatFound = true;
          newMatches += 1;
          return { ...row, checked: now, matches: [{ ...HEAT_FIND, found: now }, ...row.matches] };
        }
        return { ...row, checked: now };
      });
      return Promise.resolve({ checked, newMatches, busy: false });
    },
    wishlistMatchState: (id: number, key: string, state: WishMatchState): Promise<void> => {
      calls.wishlist.push(`match:${id}:${key}:${state}`);
      find(id);
      if (state === "dismissed") dismissed.add(`${id}:${key}`);
      else {
        dismissed.delete(`${id}:${key}`);
        replace(id, (row) => ({
          ...row,
          matches: row.matches.map((found) => (found.key === key ? { ...found, state } : found)),
        }));
      }
      return Promise.resolve();
    },
    wishlistSeen: (): Promise<number> => {
      calls.wishlist.push("seen");
      let count = 0;
      entries = entries.map((row) => ({
        ...row,
        matches: row.matches.map((found) => {
          if (found.state !== "new") return found;
          count += 1;
          return { ...found, state: "seen" };
        }),
      }));
      return Promise.resolve(count);
    },
    wishlistNewCount: (): Promise<number> =>
      Promise.resolve(
        entries
          .filter((row) => row.done === null)
          .flatMap((row) => row.matches)
          .filter((found) => found.state === "new").length,
      ),
  };
}
