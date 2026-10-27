import { ContextMenu as RadixContextMenu } from "radix-ui";
import type { KeyboardEvent, MouseEvent, ReactElement, ReactNode } from "react";

import { cx } from "../lib/cx";
import { isContextMenuKey, MenuKindContext, menuSurface, useFloatingContainer } from "./floating";
import { SurfaceContext } from "./surface";

interface ContextMenuProps {
  /** The area that has the menu (a row list, a poster grid, one tile); it must take a ref. */
  children: ReactElement;
  /** The items: `MenuItem`, `MenuSeparator`, `MenuLabel` — the same actions as the selection bar. */
  menu: ReactNode;
  "aria-label"?: string;
  /**
   * Called with the element that was right-clicked (or focused, for the keyboard) before the menu
   * opens. A list selects that item alone here when it is not already selected, so the menu acts
   * on what the user pointed at.
   */
  onTarget?: (target: Element) => void;
  /**
   * Where Shift+F10 and the menu key open the menu: the focused item by default. A list that keeps
   * focus on itself (aria-activedescendant) returns its current item here.
   */
  keyboardTarget?: () => Element | null;
  /** Turns the menu off; the browser's own menu stays off too (desktop guards). */
  disabled?: boolean;
  onOpenChange?: (open: boolean) => void;
  className?: string;
}

/**
 * Our own right-click menu (spec desktop-interaction "Own right-click menus"). It opens at the
 * pointer on right-click, and on Shift+F10 or the menu key under the focused item; `onTarget`
 * runs first so an unselected item can be selected alone. Same floating surface, item sizes and
 * key hints as `Menu`; Esc and an outside click close it.
 */
export function ContextMenu({
  children,
  menu,
  "aria-label": ariaLabel,
  onTarget,
  keyboardTarget,
  disabled,
  onOpenChange,
  className,
}: ContextMenuProps) {
  const container = useFloatingContainer();
  const onContextMenu = (event: MouseEvent) => {
    if (disabled === true) return;
    if (event.target instanceof Element) onTarget?.(event.target);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (disabled === true || !isContextMenuKey(event)) return;
    const focused =
      keyboardTarget?.() ??
      (document.activeElement instanceof Element &&
      event.currentTarget.contains(document.activeElement)
        ? document.activeElement
        : event.currentTarget);
    if (focused === null) return;
    event.preventDefault();
    const rect = focused.getBoundingClientRect();
    // Open the menu under the item's start edge, as if it had been right-clicked there.
    focused.dispatchEvent(
      new window.MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        clientX: rect.left + Math.min(24, rect.width / 2),
        clientY: rect.top + rect.height,
      }),
    );
  };

  return (
    <RadixContextMenu.Root onOpenChange={onOpenChange} modal>
      <RadixContextMenu.Trigger
        asChild
        disabled={disabled}
        onContextMenu={onContextMenu}
        onKeyDown={onKeyDown}
      >
        {children}
      </RadixContextMenu.Trigger>
      <RadixContextMenu.Portal container={container}>
        <RadixContextMenu.Content
          aria-label={ariaLabel}
          collisionPadding={8}
          className={cx(
            menuSurface,
            "max-h-(--radix-context-menu-content-available-height)",
            className,
          )}
        >
          <MenuKindContext value="context">
            <SurfaceContext value="floating">{menu}</SurfaceContext>
          </MenuKindContext>
        </RadixContextMenu.Content>
      </RadixContextMenu.Portal>
    </RadixContextMenu.Root>
  );
}
