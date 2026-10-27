import { t } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";
import type { ArtworkCandidate } from "./artworkBackend";
import { isExtraSlot, languageOf, pixels, slotLabelKey } from "./artworkModel";

/** The artwork types in the order of the type strip. */
const STRIP_ORDER: readonly string[] = [
  "poster",
  "fanart",
  "clearlogo",
  "clearart",
  "banner",
  "disc",
  "thumb",
  "keyart",
  "squareart",
  "characterart",
];

const TYPE_LABELS: Readonly<Record<string, MessageKey>> = {
  poster: "artworkPicker.type.poster",
  fanart: "artworkPicker.type.fanart",
  clearlogo: "artworkPicker.type.clearlogo",
  clearart: "artworkPicker.type.clearart",
  banner: "artworkPicker.type.banner",
  disc: "artworkPicker.type.disc",
  thumb: "artworkPicker.type.thumb",
  keyart: "artworkPicker.type.keyart",
  squareart: "artworkPicker.type.squareart",
  characterart: "artworkPicker.type.characterart",
};

/** The message that names an artwork type in the strip, or `null` for a type this build does not know. */
function typeLabelKey(slot: string): MessageKey | null {
  return Object.hasOwn(TYPE_LABELS, slot) ? (TYPE_LABELS[slot] as MessageKey) : null;
}

/**
 * The types the strip lists: the ones the title can have that are chosen one image at a time, in
 * a fixed order, then any other the backend names. The extras (several images at once) are
 * not part of this picker.
 */
export function stripSlots(slots: readonly string[]): string[] {
  const single = slots.filter((slot) => !isExtraSlot(slot));
  const known = STRIP_ORDER.filter((slot) => single.includes(slot));
  const rest = single.filter((slot) => !STRIP_ORDER.includes(slot));
  return [...known, ...rest];
}

/** The types with transparency, saved as PNG; the others are JPEG. */
const TRANSPARENT: ReadonlySet<string> = new Set(["clearlogo", "clearart", "disc", "characterart"]);

const FILE_STEMS: Readonly<Record<string, string>> = {
  clearlogo: "clearlogo",
  clearart: "clearart",
  disc: "disc",
  characterart: "characterart",
  squareart: "squareart",
};

/** The file name an image of a type is saved as: `poster.jpg`, `clearlogo.png`. */
export function savedAs(slot: string, sourceName?: string): string {
  const stem = FILE_STEMS[slot] ?? slot;
  const ext = /\.(png|jpe?g|webp|bmp)$/i.exec(sourceName ?? "")?.[1]?.toLowerCase();
  if (ext !== undefined) return `${stem}.${ext === "jpeg" ? "jpg" : ext}`;
  return `${stem}.${TRANSPARENT.has(slot) ? "png" : "jpg"}`;
}

/** How the grid is ordered. */
export type PickerSort = "votes" | "size";

/** What people think of an image: votes at TMDB, likes at Fanart.tv; 0 when nobody has. */
function popularityCount(candidate: ArtworkCandidate): number {
  return candidate.likes > 0 ? candidate.likes : candidate.votes;
}

/**
 * The candidates in the order of a sort menu: Votes puts the most voted or liked first, then the
 * backend's own order; Size puts the biggest first. The image the title has now is pinned by the
 * caller, and stills from the video and images too small for the type come last.
 */
export function sortForPicker(
  candidates: readonly ArtworkCandidate[],
  sort: PickerSort,
): ArtworkCandidate[] {
  const copy = [...candidates];
  const frames = (candidate: ArtworkCandidate) => (candidate.provider === "frames" ? 1 : 0);
  const low = (candidate: ArtworkCandidate) => (candidate.low_quality ? 1 : 0);
  copy.sort(
    (a, b) =>
      low(a) - low(b) ||
      frames(a) - frames(b) ||
      (sort === "votes"
        ? popularityCount(b) - popularityCount(a) || b.best - a.best
        : pixels(b) - pixels(a) || b.best - a.best),
  );
  return copy;
}

/** One language chip of the filter row. */
export interface LanguageChoice {
  /** A language code, {@link NO_TEXT} for textless, or `""` for All. */
  value: string;
  count: number;
}

/** All, then up to three languages that have images, the most common first (Textless among them). */
export function languageChoices(candidates: readonly ArtworkCandidate[]): LanguageChoice[] {
  const counts = new Map<string, number>();
  for (const candidate of candidates) {
    const language = languageOf(candidate);
    counts.set(language, (counts.get(language) ?? 0) + 1);
  }
  const top = [...counts.entries()]
    .sort(([a, x], [b, y]) => y - x || a.localeCompare(b))
    .slice(0, 3)
    .map(([value, count]) => ({ value, count }));
  return [{ value: "", count: candidates.length }, ...top];
}

/** Whether a candidate has the language of a chip (`""` is any). */
export function hasLanguage(candidate: ArtworkCandidate, value: string): boolean {
  return value === "" || languageOf(candidate) === value;
}

/** The name of a type in the strip: "Logo", "Clear art"; the type's own id when this build does not know it. */
export function typeName(slot: string): string {
  const key = typeLabelKey(slot) ?? slotLabelKey(slot);
  return key === null ? slot : t(key);
}
