import { isTauri } from "@tauri-apps/api/core";

import type { Channel } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  ArtworkCandidateDto,
  ArtworkCommandError,
  ArtworkStreamStartDto,
  PosterDto,
  ProviderEvent,
} from "../../bindings";
import { t } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { currentHistoryBackend, HISTORY_KEY } from "../history/historyBackend";
import { channelTransport } from "../workspaces/channelTransport";
import type { ProviderSearchTransport } from "../workspaces/providerEvents";
import { createSyntheticArtworkBackend } from "./syntheticArtwork";

/**
 * The artwork the picker works on. A slot is an artwork type such as `poster`, `fanart`,
 * `clearlogo` or `extrafanart`; the backend lists the slots an item can have.
 */
export interface ArtworkTarget {
  itemId: string;
  slot: string;
  /** The title to show before the candidates have loaded. */
  title?: string;
}

/** An image by content key, with what is known to paint it before it loads. */
export type ArtworkImage = PosterDto;

/** One image a provider offers. The image itself is already in the cache: `image.key` is a `cv://img` key. */
export interface ArtworkCandidate {
  /** Where the image comes from; passed back to place it. Unique in a list. */
  id: string;
  /** Key of the provider that offers it: `tmdb`, `fanart`, `tvdb`... */
  provider: string;
  /** Primary language of the text on the image, or `null` for an image without text. */
  language: string | null;
  /** Pixels; `0` when the provider does not say. */
  width: number;
  height: number;
  /** How well it fits the artwork settings, 0 to 100. The list arrives best first. */
  score: number;
  /**
   * The "Best" order, higher first and comparable across providers: the fit to the settings,
   * then the language, the size and how much people like the image.
   */
  best: number;
  /** How many people voted on the image at the provider (TMDB); `0` when unknown. */
  votes: number;
  /** The average of those votes times ten (`56` for 5.6); `0` without votes. */
  rating: number;
  /** How many people liked the image at the provider (Fanart.tv); `0` when unknown. */
  likes: number;
  /** Too small or the wrong shape for its type: listed behind "Show N more". */
  low_quality: boolean;
  image: ArtworkImage;
}

export interface ArtworkProviderInfo {
  key: string;
  name: string;
}

/**
 * What a started artwork search tells the workspace at once: the title, the types the item can
 * have, the providers that will answer and the image it has now for the type, pinned first.
 */
export type ArtworkStreamStart = ArtworkStreamStartDto;

/** A picture from the user's disk: a path in the desktop app, a `File` in a plain browser. */
export type LocalImage = { kind: "path"; path: string } | { kind: "file"; file: File };

/** What placing images did. */
export interface ArtworkPlaced {
  images: number;
  /** What could not be done, as `what: reason`. */
  problems: string[];
  /**
   * The History entry of the change, the token of `undoPlace`; `null` when nothing was written.
   */
  undo: string | null;
}

/** One type of an item for the picker's type strip. */
export interface ArtworkSlotCount {
  slot: string;
  /** How many images the providers offer for it. */
  count: number;
  /** The image the item has now for it. */
  current: ArtworkCandidate | null;
}

/** What filling in the missing artwork of a selection did. */
export interface ArtworkRun {
  images: number;
  portraits: number;
  /** Titles that could not be worked on. */
  skipped: number;
}

/** A refusal from the backend: `message` is ready to show, `code` says why. */
export class ArtworkRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ArtworkRequestError";
    this.code = code;
  }
}

/** Where the workspace gets its candidates and how an image is put in place. */
export interface ArtworkBackend {
  /** False when no artwork can be fetched or written here, so the entry points stay disabled. */
  readonly available: boolean;
  /**
   * The streaming search of the artwork workspace: `params` carry the `itemId` and the
   * `slot`; each provider's candidates arrive as that provider answers, already cached for
   * `cv://img`. `onStart` receives what the search tells at once.
   */
  searchTransport: (
    onStart?: (start: ArtworkStreamStart) => void,
  ) => ProviderSearchTransport<ArtworkCandidate>;
  /**
   * Sets the slot from an address: a candidate's `id`, a pasted web address, a `file:` address or
   * a path on disk. The image is written with the configured file names.
   */
  place: (itemId: string, slot: string, source: string) => Promise<ArtworkPlaced>;
  /** Sets the slot from a file the user dropped. */
  placeLocal: (itemId: string, slot: string, image: LocalImage) => Promise<ArtworkPlaced>;
  /** Replaces the extra fanart or extra thumbs with the chosen images. */
  placeExtras: (itemId: string, slot: string, sources: readonly string[]) => Promise<ArtworkPlaced>;
  /**
   * Puts back what a placement displaced: the new files go to the trash folder and the old ones
   * return. The token comes from the placement and works once.
   */
  undoPlace: (token: string) => Promise<ArtworkPlaced>;
  /**
   * How many images each type of the item has at the providers and the image it has now, without
   * searching the types one by one. `null` for an item whose types are not counted this way (a
   * season or an episode): the picker then searches each type itself.
   */
  counts: (itemId: string) => Promise<ArtworkSlotCount[] | null>;
  /** Fills the empty artwork types of the items from the providers. */
  downloadMissing: (itemIds: readonly string[]) => Promise<ArtworkRun>;
  /**
   * After episodes were scraped: fills the missing season images and episode thumbs of the show
   * of `itemId`, when the after-scrape artwork setting is on. `season` limits it to one season;
   * without it a season or an episode stands for its own season and the show for all of them.
   * Nothing that is there is replaced.
   */
  fillSeasonArtwork: (itemId: string, season: number | null) => Promise<ArtworkRun>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: ArtworkCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new ArtworkRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

/**
 * The backend of the desktop app: the providers, the image cache and the files are in the Rust
 * service, and images reach the interface only as `cv://img` keys.
 */
const tauriArtworkBackend: ArtworkBackend = {
  available: true,
  searchTransport: artworkStreamTransport,
  place: async (itemId, slot, source) => unwrap(await commands.placeArtwork(itemId, slot, source)),
  // A picture dropped on the desktop window arrives as a path, never as bytes.
  placeLocal: async (itemId, slot, image) => {
    if (image.kind !== "path") {
      throw new ArtworkRequestError("unsupported", t("artwork.unavailable"));
    }
    return unwrap(await commands.placeArtwork(itemId, slot, image.path));
  },
  placeExtras: async (itemId, slot, sources) =>
    unwrap(await commands.placeExtraArtwork(itemId, slot, [...sources])),
  // A placement is a History entry: its token is the entry's id, and Undo is History's.
  undoPlace: async (token) => {
    const report = await currentHistoryBackend().undo(token);
    void queryClient.invalidateQueries({ queryKey: HISTORY_KEY });
    const problems = report.left.map((entry) => `${entry.title}: ${entry.reason}`);
    if (report.undone.length === 0) throw new Error(problems.join(" "));
    return { images: report.undone.length, problems, undo: null };
  },
  counts: async (itemId) => {
    const result = await commands.artworkCandidateCounts(itemId);
    // A season or an episode is not counted this way.
    if (result.status === "error" && result.error.code === "unsupported") return null;
    return unwrap(result);
  },
  downloadMissing: async (itemIds) => unwrap(await commands.downloadMissingArtwork([...itemIds])),
  fillSeasonArtwork: async (itemId, season) =>
    unwrap(await commands.fillSeasonArtwork(itemId, season)),
};

/** The provider of the image the item has now, which a stream pins first. */
export const CURRENT_PROVIDER = "current";

/**
 * The streaming search of the artwork workspace: `params` of the request carry the
 * `itemId` and the `slot`. Each provider's candidates arrive on the Channel as that provider
 * answers; `onStart` receives what the search tells at once, the providers that will answer and
 * the current image, pinned first.
 */
function artworkStreamTransport(
  onStart?: (start: ArtworkStreamStartDto) => void,
): ProviderSearchTransport<ArtworkCandidate> {
  return channelTransport<ArtworkCandidate>(async (request, channel) => {
    const start = unwrap(
      await commands.streamArtworkCandidates(
        String(request.params?.itemId ?? ""),
        String(request.params?.slot ?? ""),
        request.searchId,
        channel as Channel<ProviderEvent<ArtworkCandidateDto>>,
      ),
    );
    onStart?.(start);
  });
}

let override: ArtworkBackend | null = null;
let fallback: ArtworkBackend | null = null;
/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setArtworkBackend(backend: ArtworkBackend | null): void {
  override = backend;
}

export function currentArtworkBackend(): ArtworkBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriArtworkBackend;
  // A plain browser has no providers: it uses fixtures so the picker can be developed.
  fallback ??= createSyntheticArtworkBackend();
  return fallback;
}
