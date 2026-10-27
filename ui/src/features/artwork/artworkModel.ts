import type { MessageKey } from "../../i18n/catalog";
import type { ArtworkCandidate } from "./artworkBackend";

/** The slots whose images are chosen several at a time and replace the whole set. */
const EXTRA_SLOTS: ReadonlySet<string> = new Set(["extrafanart", "extrathumb"]);

export function isExtraSlot(slot: string): boolean {
  return EXTRA_SLOTS.has(slot);
}

const SLOT_LABELS: Readonly<Record<string, MessageKey>> = {
  poster: "artwork.slot.poster",
  fanart: "artwork.slot.fanart",
  banner: "artwork.slot.banner",
  clearart: "artwork.slot.clearart",
  clearlogo: "artwork.slot.clearlogo",
  disc: "artwork.slot.disc",
  thumb: "artwork.slot.thumb",
  keyart: "artwork.slot.keyart",
  squareart: "artwork.slot.squareart",
  characterart: "artwork.slot.characterart",
  extrafanart: "artwork.slot.extrafanart",
  extrathumb: "artwork.slot.extrathumb",
};

/** The message that names a slot, or `null` for a slot this build does not know. */
export function slotLabelKey(slot: string): MessageKey | null {
  return Object.hasOwn(SLOT_LABELS, slot) ? (SLOT_LABELS[slot] as MessageKey) : null;
}

const TALL: ReadonlySet<string> = new Set(["poster", "keyart"]);
const SQUARE: ReadonlySet<string> = new Set(["disc", "squareart"]);
/** Images with transparency or a fixed shape are shown whole instead of cropped. */
const CONTAIN: ReadonlySet<string> = new Set(["clearlogo", "clearart", "disc", "characterart"]);

/** The CSS aspect ratio a card of a slot has. */
export function slotAspect(slot: string): string {
  if (TALL.has(slot)) return "2 / 3";
  if (SQUARE.has(slot)) return "1 / 1";
  if (slot === "banner") return "758 / 140";
  return "16 / 9";
}

export function slotFitsWhole(slot: string): boolean {
  return CONTAIN.has(slot);
}

export type SortKey = "best" | "size";

/** Values of the language filter that are not language codes. */
export const ANY_LANGUAGE = "";
export const NO_TEXT = "none";

export interface CandidateFilters {
  /** A provider key, or an empty string for any provider. */
  provider: string;
  /** A language code, {@link NO_TEXT} or {@link ANY_LANGUAGE}. */
  language: string;
  /** Candidates narrower than this many pixels are hidden; `0` shows all. An unknown size is kept. */
  minWidth: number;
}

export function languageOf(candidate: ArtworkCandidate): string {
  return candidate.language === null || candidate.language === "" ? NO_TEXT : candidate.language;
}

export function pixels(candidate: ArtworkCandidate): number {
  return candidate.width * candidate.height;
}

/** The provider key of the stills taken from the title's own video (frame grabs). */
export const FRAMES_PROVIDER = "frames";

/**
 * The candidates the grid shows at first and how many it keeps behind "Show N more": the images
 * that are too small or misshapen, unless there is nothing else (or `all` is asked for).
 */
export function withoutLowQuality(
  candidates: readonly ArtworkCandidate[],
  all: boolean,
): { shown: readonly ArtworkCandidate[]; hidden: number } {
  const good = candidates.filter((candidate) => !candidate.low_quality);
  if (all || good.length === 0) return { shown: candidates, hidden: 0 };
  return { shown: good, hidden: candidates.length - good.length };
}

/** What people think of an image at its provider, for the quiet hint on its tile. */
export type Popularity =
  { kind: "votes"; count: number; rating: number } | { kind: "likes"; count: number };

/** The popularity of a candidate, or `null` when its provider says nothing (or nobody voted). */
export function popularityOf(
  candidate: Pick<ArtworkCandidate, "votes" | "rating" | "likes">,
): Popularity | null {
  if (candidate.likes > 0) return { kind: "likes", count: candidate.likes };
  if (candidate.votes > 0) {
    return { kind: "votes", count: candidate.votes, rating: candidate.rating / 10 };
  }
  return null;
}

/**
 * The candidates to show while the providers answer: the stills from the video wait until every
 * online provider has answered, because the online candidates come first and would otherwise be
 * inserted above them, moving the tile under the pointer (QA final-26). Asking for Frames alone
 * shows them at once.
 */
export function withFramesSettled(
  candidates: readonly ArtworkCandidate[],
  onlineSearching: boolean,
  provider: string,
): readonly ArtworkCandidate[] {
  if (!onlineSearching || provider === FRAMES_PROVIDER) return candidates;
  return candidates.filter((candidate) => candidate.provider !== FRAMES_PROVIDER);
}

/** `2000×3000`, or an empty string when the provider does not say. */
export function dimensions(candidate: Pick<ArtworkCandidate, "width" | "height">): string {
  return candidate.width > 0 && candidate.height > 0
    ? `${candidate.width}×${candidate.height}`
    : "";
}

const IMAGE_EXTENSIONS: ReadonlySet<string> = new Set(["jpg", "jpeg", "png", "webp", "bmp"]);

function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot < 0 ? "" : name.slice(dot + 1).toLowerCase();
}

/** Whether a file name is one of the picture types an artwork file can have. */
export function isImageName(name: string): boolean {
  return IMAGE_EXTENSIONS.has(extensionOf(name.split(/[?#]/)[0] ?? ""));
}

export type SourceCheck = { ok: true; source: string } | { ok: false };

/**
 * Checks a pasted address: a web address, a `file:` address or a path on disk (`C:\Art\a.jpg`,
 * `\\server\share\a.png`, `/home/me/a.webp`, `~/a.jpg`). Web addresses are not required to end in
 * an image extension, since many image hosts do not; a path or `file:` address is.
 */
export function checkSource(text: string): SourceCheck {
  const source = text.trim().replace(/^["']|["']$/g, "");
  if (source === "") return { ok: false };
  if (/^https?:\/\/\S+$/i.test(source)) {
    try {
      return new URL(source).hostname === "" ? { ok: false } : { ok: true, source };
    } catch {
      return { ok: false };
    }
  }
  const local =
    /^file:/i.test(source) || /^[a-z]:[\\/]/i.test(source) || /^(\\\\|\/|~\/)/.test(source);
  return local && isImageName(source) ? { ok: true, source } : { ok: false };
}

/** The fit of an image (0 to 90) in words: a bare number says little to a user. */
export function fitKey(score: number): MessageKey {
  if (score >= 70) return "artwork.fit.best";
  if (score >= 40) return "artwork.fit.good";
  return "artwork.fit.loose";
}
