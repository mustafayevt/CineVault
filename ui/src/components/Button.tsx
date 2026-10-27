import type { LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";

import { cx } from "../lib/cx";
import { buttonBase, buttonDisabledLooks, buttonLooks, buttonSizes } from "./buttonStyles";
import type { ButtonSize, ButtonVariant } from "./buttonStyles";
import { Icon } from "./Icon";
import { useDisabledReason } from "./useDisabledReason";
import type { DisabledReasonProps } from "./useDisabledReason";

export type { ButtonSize, ButtonVariant } from "./buttonStyles";
export { IconButton } from "./IconButton";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement>, DisabledReasonProps {
  /** `primary` (gold, one per region), `default`, `ghost` or `danger`. */
  variant?: ButtonVariant;
  /** `sm` 24, `md` 28 (default), `lg` 32 (dialog and commit buttons only). */
  size?: ButtonSize;
  /** Leading icon, drawn at 16px. */
  icon?: LucideIcon;
  /** Trailing icon, drawn at 16px after the label (a chevron on a button that opens a menu). */
  trailingIcon?: LucideIcon;
  /** Keyboard shortcut shown after the label, for example `E` or `Enter`. */
  shortcut?: string;
  ref?: Ref<HTMLButtonElement>;
  children: ReactNode;
}

/** The one button: primary (gold), default, ghost and danger at three heights. */
export function Button({
  variant = "default",
  size = "md",
  icon,
  trailingIcon,
  shortcut,
  className,
  type = "button",
  children,
  ref,
  disabled,
  disabledReason,
  enabledBy,
  onClick,
  onFocus,
  onBlur,
  onMouseEnter,
  onMouseLeave,
  onKeyDown,
  ...rest
}: ButtonProps) {
  const look = variant;
  const reason = useDisabledReason({ disabledReason, enabledBy });
  const control = reason.controlProps;
  const inactive = reason.disabled || disabled === true;
  const button = (
    <button
      ref={ref}
      type={type}
      disabled={disabled}
      className={cx(
        buttonBase,
        inactive ? buttonDisabledLooks[look] : buttonLooks[look],
        buttonSizes[size],
        className,
      )}
      {...rest}
      {...control}
      // A control with a reason stays focusable but ignores activation.
      onClick={reason.disabled ? (event) => event.preventDefault() : onClick}
      onFocus={(event) => {
        control.onFocus?.();
        onFocus?.(event);
      }}
      onBlur={(event) => {
        control.onBlur?.();
        onBlur?.(event);
      }}
      onMouseEnter={(event) => {
        control.onMouseEnter?.();
        onMouseEnter?.(event);
      }}
      onMouseLeave={(event) => {
        control.onMouseLeave?.();
        onMouseLeave?.(event);
      }}
      onKeyDown={(event) => {
        control.onKeyDown?.(event);
        onKeyDown?.(event);
      }}
    >
      {icon && <Icon icon={icon} />}
      <span>{children}</span>
      {trailingIcon && <Icon icon={trailingIcon} />}
      {shortcut && (
        <kbd
          className="ml-0.5 text-(length:--fs-11) font-normal tabular-nums opacity-70"
          aria-hidden="true"
        >
          {shortcut}
        </kbd>
      )}
    </button>
  );
  if (!reason.disabled) return button;
  return (
    <span className="relative inline-flex shrink-0">
      {button}
      {reason.tip}
    </span>
  );
}
