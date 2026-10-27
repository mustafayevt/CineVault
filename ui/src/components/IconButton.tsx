import type { LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes, Ref } from "react";

import { cx } from "../lib/cx";
import { buttonBase, buttonDisabledLooks, buttonLooks, iconButtonSizes } from "./buttonStyles";
import type { ButtonSize, ButtonVariant } from "./buttonStyles";
import { Icon } from "./Icon";
import { Tooltip } from "./Tooltip";

type IconButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "aria-label" | "children"> & {
  /** Required: an icon-only control has no other accessible name. It is also the tooltip. */
  "aria-label": string;
  icon: LucideIcon;
  /** `ghost` by default; `primary` is not offered for icon-only buttons. */
  variant?: Exclude<ButtonVariant, "primary">;
  /** `md` 28 x 28 (default), `sm` 24, `lg` 32. */
  size?: ButtonSize;
  /** Marks a toggle button as pressed (active fill, `aria-pressed`). */
  pressed?: boolean;
  ref?: Ref<HTMLButtonElement>;
};

/**
 * A square icon-only button, 28 x 28 with a 16px icon. The required `aria-label` is also its
 * Tooltip (after 500 ms of rest, at once on keyboard focus); a `title`, when given, replaces the
 * tooltip text and is never set as a native title (that would be a second, browser-drawn tooltip).
 */
export function IconButton({
  icon,
  variant = "ghost",
  size = "md",
  pressed,
  className,
  type = "button",
  title,
  disabled,
  ref,
  "aria-label": ariaLabel,
  ...rest
}: IconButtonProps) {
  const look = variant;
  return (
    <Tooltip content={title ?? ariaLabel}>
      <button
        ref={ref}
        type={type}
        aria-label={ariaLabel}
        aria-pressed={pressed}
        disabled={disabled}
        className={cx(
          buttonBase,
          disabled ? buttonDisabledLooks[look] : buttonLooks[look],
          iconButtonSizes[size],
          pressed && !disabled && "bg-surface-active text-text",
          className,
        )}
        {...rest}
      >
        <Icon icon={icon} />
      </button>
    </Tooltip>
  );
}
