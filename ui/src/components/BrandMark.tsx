import { cx } from "../lib/cx";
import { SYMBOL, WORDMARK } from "./brandPaths";

interface BrandMarkProps {
  /** `lockup` is the symbol and the wordmark; `symbol` is the symbol alone (the collapsed rail). */
  variant?: "lockup" | "symbol";
  /** Height of the symbol in px; the wordmark is scaled to match. */
  size?: number;
  /**
   * Accessible name, for a mark that stands alone (onboarding). Leave it out next to visible text
   * or a labelled control: the mark is then hidden from assistive technology.
   */
  label?: string;
  className?: string;
}

/** Wordmark cap height as a share of the symbol's height, so the letters sit at the ring's middle. */
const CAP_TO_SYMBOL = 0.6;
/** Cap height of the wordmark in its own units (Archivo SemiExpanded at 96 units). */
const WORDMARK_CAP = 65.86;

/**
 * The CineVault brand mark (assets/brand/): a C-shaped ring with round terminals and a 2:3
 * frame filed in its opening, then the wordmark whose i takes the same frame as its dot. The ring
 * and the letters follow the current text colour; the frame and the tittle are the accent. Flat
 * colour only.
 */
export function BrandMark({ variant = "lockup", size = 18, label, className }: BrandMarkProps) {
  const a11y = label
    ? ({ role: "img", "aria-label": label } as const)
    : ({ "aria-hidden": true } as const);
  const symbol = (
    <svg
      width={size}
      height={size}
      viewBox={SYMBOL.viewBox}
      focusable="false"
      className="shrink-0"
      data-testid="brand-symbol"
      {...(variant === "symbol" ? a11y : { "aria-hidden": true })}
    >
      <path d={SYMBOL.ring} fill="currentColor" />
      <path d={SYMBOL.frame} className="fill-gold" />
    </svg>
  );
  if (variant === "symbol") {
    return <span className={cx("inline-flex", className)}>{symbol}</span>;
  }
  const scale = (size * CAP_TO_SYMBOL) / WORDMARK_CAP;
  return (
    <span
      className={cx("inline-flex items-center", className)}
      style={{ gap: size * 0.45 }}
      {...a11y}
    >
      {symbol}
      <svg
        width={Math.round(WORDMARK.width * scale * 10) / 10}
        height={Math.round(WORDMARK.height * scale * 10) / 10}
        viewBox={WORDMARK.viewBox}
        focusable="false"
        aria-hidden="true"
        data-testid="brand-wordmark"
      >
        <path d={WORDMARK.letters} fill="currentColor" />
        <path d={WORDMARK.tittle} className="fill-gold" />
      </svg>
    </span>
  );
}
