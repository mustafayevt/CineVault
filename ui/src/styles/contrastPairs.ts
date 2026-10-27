/**
 * Foreground/background token pairs the base components actually use, with the WCAG 2.1 AA
 * threshold each must meet. `a11y.contrast.test.ts` checks them in both themes.
 *
 * - `fg` and `ground` name colour tokens (`--focus-ring` is resolved through its `var()`).
 * - `soft` names a translucent tint (`*-soft`) that is composited over `ground` first.
 * - `fgOpacity` is the opacity applied to the foreground (the 70% shortcut hints).
 * - `min` is 4.5 for text and 3 for large text and UI components such as focus rings.
 */
export interface ContrastPair {
  usage: string;
  fg: string;
  ground: string;
  soft?: string;
  fgOpacity?: number;
  min: 2.5 | 3 | 4.5;
}

/** The surface ladder: every ground text can sit on (the well only inside floating surfaces). */
const ladder = [
  "surface-window",
  "surface-card",
  "surface-well",
  "surface-raised",
  "surface-active",
  "surface-pressed",
  "surface-floating",
  "surface-floating-hover",
];

/** Grounds of quiet text: page, cards, docked panes and floating surfaces (not buttons or hover). */
const quietGrounds = ["surface-window", "surface-card", "surface-well", "surface-floating"];

function onGrounds(
  usage: string,
  fg: string,
  grounds: string[],
  min: ContrastPair["min"] = 4.5,
  extra: Partial<ContrastPair> = {},
): ContrastPair[] {
  return grounds.map((ground) => ({ usage: `${usage} on ${ground}`, fg, ground, min, ...extra }));
}

export const CONTRAST_PAIRS: ContrastPair[] = [
  ...onGrounds("body text", "text", ladder),
  ...onGrounds("secondary text", "text-2", ladder),
  // Muted text: counts, meta lines, section labels, table headers. On the dark raised and active
  // steps it falls just under 4.5:1, so buttons and active items use text or text-2 instead.
  ...onGrounds("muted text", "text-muted", quietGrounds),
  // Selected rows: the gold tint over the card, with every text colour on top.
  ...(["text", "text-2"] as const).flatMap((fg) =>
    onGrounds(`${fg} on a selected row`, fg, ["surface-card", "surface-window"], 4.5, {
      soft: "selected-row",
    }),
  ),
  // Disabled text is exempt from WCAG 1.4.3; it keeps 2.5:1 so it stays legible, without opacity.
  ...onGrounds("disabled text", "text-disabled", ["surface-card", "surface-raised"], 2.5),
  // Gold as a mark (logo, active page icon, progress, checks) and as a focused field's border.
  ...onGrounds("gold mark", "gold", ["surface-window", "surface-card", "surface-floating"], 3),
  { usage: "gold text on the window", fg: "gold", ground: "surface-window", min: 4.5 },
  { usage: "primary button label", fg: "on-gold", ground: "gold", min: 4.5 },
  { usage: "primary button hover", fg: "on-gold", ground: "gold-hover", min: 4.5 },
  // Active chip: text on the gold tint.
  ...onGrounds("active chip label", "text", ["surface-window", "surface-card"], 4.5, {
    soft: "gold-tint",
  }),
  // State words next to their glyphs.
  ...(["ok", "warn", "danger"] as const).flatMap((tone) =>
    onGrounds(`${tone} text`, tone, [...quietGrounds, "surface-active"]),
  ),
  ...onGrounds("warning tag", "warn", ["surface-card", "surface-window"], 4.5, {
    soft: "warn-tint",
  }),
  // Bars against their track and the grounds around them.
  ...onGrounds("warning bar", "warn-bar", ["surface-card", "surface-floating"], 3),
  { usage: "text over artwork on the scrim", fg: "scrim-text", ground: "scrim", min: 4.5 },
  // Non-text contrast: the focus ring must reach 3:1 against every ground a control can sit on.
  ...onGrounds("focus ring", "focus-ring", ladder, 3),
];

/**
 * Pairs that the design tokens, as published, do not bring to the threshold. Components avoid them
 * (or pair them with text that carries the meaning). They are recorded here so the gap stays visible;
 * the test asserts each is still below its threshold, so it fails when the tokens are fixed and the
 * entry can be deleted.
 */
export const KNOWN_TOKEN_SHORTFALLS: (ContrastPair & {
  themes: ("dark" | "light")[];
  note: string;
})[] = [
  {
    usage: "muted text on a selected row",
    fg: "text-muted",
    ground: "surface-card",
    soft: "selected-row",
    min: 4.5,
    themes: ["dark"],
    note: "Cells of a selected row use --text or --text-2; muted text is not drawn on the selection.",
  },
];
