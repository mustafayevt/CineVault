/**
 * The one button family: 24 / 28 / 32 px, radius 6, 13/500, icon 16
 * with a 6px gap. Shared by Button and IconButton so the two cannot drift. Colours come from the
 * surface ladder; hover and pressed are the next two rungs, never opacity.
 */

/** The four looks: gold `primary` (one per region), `default`, `ghost` and `danger`. */
export type ButtonVariant = "primary" | "default" | "ghost" | "danger";

export type ButtonLook = ButtonVariant;

/** small 24 (12px text), regular 28, large 32 (dialogs and commit steps only). */
export type ButtonSize = "sm" | "md" | "lg";

export const buttonBase =
  "cv-hoverable inline-flex shrink-0 items-center justify-center gap-1.5 whitespace-nowrap " +
  "rounded-(--r-6) border border-solid font-(weight:--fw-500) leading-none no-underline " +
  "[&_svg]:size-4 [&_svg]:shrink-0";

// Written out in full: Tailwind finds classes by scanning for whole strings, so they cannot be
// assembled from parts. Hover and pressed apply only to a control that can act.
export const buttonLooks: Record<ButtonLook, string> = {
  default:
    "border-line-edge bg-surface-raised text-text " +
    "enabled:not-aria-disabled:hover:bg-surface-active enabled:not-aria-disabled:active:bg-surface-pressed",
  primary:
    "border-gold bg-gold font-(weight:--fw-600) text-on-gold " +
    "enabled:not-aria-disabled:hover:border-gold-hover enabled:not-aria-disabled:hover:bg-gold-hover " +
    "enabled:not-aria-disabled:active:brightness-95",
  ghost:
    "border-transparent bg-transparent text-text-2 " +
    "enabled:not-aria-disabled:hover:bg-surface-active enabled:not-aria-disabled:hover:text-text " +
    "enabled:not-aria-disabled:active:bg-surface-pressed",
  danger:
    "border-line-edge bg-surface-raised text-danger " +
    "enabled:not-aria-disabled:hover:bg-surface-active enabled:not-aria-disabled:active:bg-surface-pressed",
};

/** Disabled: the same shape in the disabled text colour, never a faded block. */
export const buttonDisabledLooks: Record<ButtonLook, string> = {
  default: "border-line-edge bg-surface-raised text-text-disabled",
  primary: "border-line-edge bg-surface-raised text-text-disabled",
  ghost: "border-transparent bg-transparent text-text-disabled",
  danger: "border-line-edge bg-surface-raised text-text-disabled",
};

export const buttonSizes: Record<ButtonSize, string> = {
  sm: "h-(--h-btn-sm) px-[9px] text-(length:--fs-12)",
  md: "h-(--h-btn) px-3 text-(length:--fs-13)",
  lg: "h-(--h-btn-lg) px-3.5 text-(length:--fs-13)",
};

/** Icon-only buttons are square at the button height. */
export const iconButtonSizes: Record<ButtonSize, string> = {
  sm: "size-(--h-btn-sm) p-0",
  md: "size-(--h-btn) p-0",
  lg: "size-(--h-btn-lg) p-0",
};
