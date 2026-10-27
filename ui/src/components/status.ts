/** The one status vocabulary: a glyph and a word, never colour alone. */
export type Status = "done" | "waiting" | "missing" | "needs-key" | "in-progress" | "off";

export const STATUSES: readonly Status[] = [
  "done",
  "waiting",
  "missing",
  "needs-key",
  "in-progress",
  "off",
];
