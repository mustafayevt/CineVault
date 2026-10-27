import type { LucideIcon } from "lucide-react";
import { ContextMenu, DropdownMenu } from "radix-ui";
import { useContext } from "react";
import type { ComponentProps } from "react";

import { cx } from "../lib/cx";
import { menuItemClass, MenuKindContext } from "./floating";
import { Icon } from "./Icon";
import { Kbd } from "./Kbd";
import { useDisabledReason } from "./useDisabledReason";
import type { DisabledReasonProps } from "./useDisabledReason";

type MenuItemProps = Omit<ComponentProps<typeof DropdownMenu.Item>, "disabled"> &
  DisabledReasonProps & {
    /** A hard disable with no explanation (unreachable by keyboard). Prefer `disabledReason`. */
    disabled?: boolean;
    /** A 16px icon before the label. */
    icon?: LucideIcon;
    /** The key hint at the right edge, for example `Enter`, `F2` or `Shift Del`. */
    shortcut?: string;
    /** A permanent or destructive action: the label and icon take the danger colour. */
    danger?: boolean;
  };

/**
 * An item of a `Menu` or `ContextMenu` (it also works inside a raw Radix DropdownMenu): 28px, an
 * optional 16px icon, the label, and a key hint at the right. With `disabledReason` it stays
 * focusable and reachable with the arrow keys, ignores selection (the menu stays open) and shows
 * the one-line reason and the action that enables it on hover and focus.
 */
export function MenuItem({
  disabledReason,
  enabledBy,
  disabled,
  icon,
  shortcut,
  danger,
  onSelect,
  onFocus,
  onBlur,
  onPointerEnter,
  onPointerLeave,
  onKeyDown,
  className,
  children,
  ...rest
}: MenuItemProps) {
  const kind = useContext(MenuKindContext);
  const reason = useDisabledReason({ disabledReason, enabledBy });
  const control = reason.controlProps;
  const inert = reason.disabled || disabled === true;
  const Item = kind === "context" ? ContextMenu.Item : DropdownMenu.Item;
  return (
    <Item
      {...rest}
      disabled={disabled}
      aria-disabled={control["aria-disabled"]}
      aria-describedby={control["aria-describedby"]}
      className={cx(
        menuItemClass,
        danger && "text-danger",
        inert && "cursor-not-allowed text-text-disabled",
        className,
      )}
      onSelect={reason.disabled ? (event) => event.preventDefault() : onSelect}
      onFocus={(event) => {
        control.onFocus?.();
        onFocus?.(event);
      }}
      onBlur={(event) => {
        control.onBlur?.();
        onBlur?.(event);
      }}
      onPointerEnter={(event) => {
        control.onMouseEnter?.();
        onPointerEnter?.(event);
      }}
      onPointerLeave={(event) => {
        control.onMouseLeave?.();
        onPointerLeave?.(event);
      }}
      onKeyDown={(event) => {
        control.onKeyDown?.(event);
        onKeyDown?.(event);
      }}
    >
      {icon !== undefined && <Icon icon={icon} />}
      {children}
      {shortcut !== undefined && <Kbd className="ml-auto">{shortcut}</Kbd>}
      {reason.tip}
    </Item>
  );
}
