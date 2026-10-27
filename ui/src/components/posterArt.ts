/**
 * Generated art for titles without artwork: typographic posters, backdrops and episode stills.
 *
 * A title is hashed (FNV-1a) to pick a palette pair (ground and ink) and a motif, so the same title
 * always gets the same poster in the grid, the list thumbnail and the detail hero. The palettes are
 * picture data, not interface colour: like the rating-source brand colours they are the documented
 * exception to "colours only in tokens.css".
 */

/** Ground (c1) and ink (c2) of one generated poster. */
export type Palette = readonly [ground: string, ink: string];

const PALETTES: readonly Palette[] = [
  ["#2b1d12", "#d9a066"],
  ["#1a1210", "#f2a93b"],
  ["#2a3550", "#f0c9a8"],
  ["#3b1a0a", "#f28c28"],
  ["#e9e4d8", "#3a3a36"],
  ["#f3efe8", "#0e2f6b"],
  ["#1c2a3d", "#f3c46b"],
  ["#20332a", "#c8dcb0"],
  ["#0f1a2e", "#6fa0d8"],
  ["#160b1f", "#ff5fa6"],
  ["#2e6c8f", "#ffd9a0"],
  ["#3a3127", "#e9d8b8"],
  ["#0b0d12", "#9fb0c8"],
  ["#1f5e3a", "#f5f2e6"],
  ["#1d2b24", "#c9d6c1"],
  ["#1e2d3a", "#e36f3c"],
  ["#2b2f33", "#b7c5d3"],
  ["#101214", "#d8d8d4"],
  ["#3a2a24", "#ff7f4a"],
  ["#2a1e4e", "#f7c948"],
  ["#0f1a24", "#8ed0e8"],
  ["#2a1515", "#e6c8a0"],
  ["#1c2e6e", "#f2f2f0"],
  ["#3b3a33", "#d9cfb0"],
  ["#262118", "#f0b35a"],
  ["#232824", "#a6b4a0"],
  ["#0d1014", "#4f6b8f"],
  ["#0b0f12", "#cf3838"],
  ["#e8ebee", "#1a1a1a"],
  ["#4a2a0a", "#f6c85f"],
];

const MOTIFS = [
  "sun",
  "ring",
  "horizon",
  "haze",
  "wall",
  "fall",
  "orb",
  "leaves",
  "city",
  "neon",
  "snow",
  "shadow",
  "court",
  "stairs",
  "flame",
  "pod",
  "beam",
] as const;

export type Motif = (typeof MOTIFS)[number];

/** 32-bit FNV-1a of a string, stable across runs and platforms. */
function hashTitle(text: string): number {
  let hash = 0x811c9dc5;
  for (const char of text.normalize("NFC").toLowerCase()) {
    hash ^= char.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash >>> 0;
}

/** The title as printed on a placeholder: no trailing year, no surrounding space. */
export function posterTitle(title: string): string {
  return title
    .replace(/\s*[([]\d{4}[)\]]\s*$/, "")
    .replace(/\s+/g, " ")
    .trim();
}

export interface PosterArt {
  palette: Palette;
  motif: Motif;
  /** Caption ink: light on a dark ground, near-black on a light one. */
  ink: string;
  dark: boolean;
}

/** The palette, motif and caption ink of a title. */
export function posterArt(title: string): PosterArt {
  const hash = hashTitle(posterTitle(title) || title);
  const palette: Palette = PALETTES[hash % PALETTES.length] ?? ["#202024", "#aaa8a2"];
  const motif = MOTIFS[Math.floor(hash / PALETTES.length) % MOTIFS.length] ?? "sun";
  const dark = luminance(palette[0]) < 0.5;
  return { palette, motif, ink: dark ? "#f3efe8" : "#161616", dark };
}

/** Relative luminance (0-1, perceptual weights, no gamma) of a #rrggbb colour. */
function luminance(hex: string): number {
  const n = Number.parseInt(hex.slice(1), 16);
  return (0.2126 * ((n >> 16) & 255) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255)) / 255;
}

/** Lightens (k > 0, towards white) or darkens (k < 0, towards black) a #rrggbb colour. */
export function shade(hex: string, k: number): string {
  const n = Number.parseInt(hex.slice(1), 16);
  const step = (v: number) =>
    Math.max(0, Math.min(255, Math.round(k < 0 ? v * (1 + k) : v + (255 - v) * k)));
  return `#${[(n >> 16) & 255, (n >> 8) & 255, n & 255]
    .map(step)
    .map((v) => v.toString(16).padStart(2, "0"))
    .join("")}`;
}

/** Longest line, in characters, that fits the poster width at the large caption size. */
const LINE_CHARS = 11;

/**
 * Breaks a title into at most three balanced lines for the poster caption. Words longer than a
 * line are hyphenated near the middle ("Oppen-", "heimer"); a title that still needs more lines
 * is cut with an ellipsis. `small` is true when a line runs long and the caption steps down.
 */
export function posterLines(title: string): { lines: string[]; small: boolean } {
  const words = posterTitle(title)
    .split(" ")
    .filter(Boolean)
    .flatMap((word) => {
      if ([...word].length <= LINE_CHARS + 2) return [word];
      const chars = [...word];
      const cut = Math.ceil(chars.length / 2);
      return [`${chars.slice(0, cut).join("")}-`, chars.slice(cut).join("")];
    });
  const lines: string[] = [];
  for (const word of words) {
    const last = lines.at(-1);
    if (last !== undefined && !last.endsWith("-") && [...`${last} ${word}`].length <= LINE_CHARS) {
      lines[lines.length - 1] = `${last} ${word}`;
    } else {
      lines.push(word);
    }
  }
  const small = lines.length > 2 || lines.some((line) => [...line].length > LINE_CHARS);
  if (lines.length > 3) {
    const kept = lines.slice(0, 3);
    kept[2] = `${kept[2]?.replace(/-$/, "") ?? ""}…`;
    return { lines: kept, small: true };
  }
  return { lines: lines.length > 0 ? lines : [title], small };
}

/** Initials of a person's name, for a portrait that has no picture. */
export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .map((part) => [...part][0] ?? "")
    .slice(0, 2)
    .join("")
    .toUpperCase();
}
