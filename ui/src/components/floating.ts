import { createContext, useContext } from "react";

import { cx } from "../lib/cx";

/**
 * The one floating recipe: the floating surface, a `--line-edge`
 * border, the floating shadow, and `.cv-pop` so it fades in and grows from 0.98 at its anchor.
 * Menus, popovers, tooltips and dialogs all start from it; only the radius and padding differ.
 */
export const floatingSurface = cx(
  "cv-pop z-50 border border-solid border-line-edge bg-surface-floating text-text shadow-floating",
  "outline-none",
);

/** A menu: padding 4, radius 10, at least 264 wide. */
export const menuSurface = cx(
  floatingSurface,
  "min-w-[264px] overflow-y-auto rounded-(--r-10) p-1 text-(length:--fs-13)",
);

/** A menu item: 28px, 13px, gap 10, radius 6, the active rung on hover and keyboard highlight. */
export const menuItemClass = cx(
  "cv-hoverable relative flex h-(--h-menu-item) cursor-pointer items-center gap-2.5 rounded-(--r-6) px-2.5",
  "text-(length:--fs-13) whitespace-nowrap text-text outline-none select-none",
  "data-[highlighted]:bg-surface-active",
  "[&>svg]:size-4 [&>svg]:shrink-0",
);

/** Which Radix menu family an item belongs to: a drop-down menu or a right-click menu. */
export type MenuKind = "dropdown" | "context";

/**
 * Set by `Menu` and `ContextMenu` so one `MenuItem` works in both. Left out (an item inside a raw
 * Radix DropdownMenu) it means a drop-down menu.
 */
export const MenuKindContext = createContext<MenuKind>("dropdown");

/** Shift+F10 or the context-menu key: the keyboard way to ask for a right-click menu. */
export function isContextMenuKey(event: { key: string; shiftKey: boolean }): boolean {
  return event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey);
}

/**
 * Where floating surfaces are drawn. By default the end of `<body>`; the component gallery sets the
 * element of each theme column, so a menu opened in the light column is light too (theme variables
 * come from the nearest `data-theme`, and a portal in `<body>` would take the window's).
 */
export const FloatingContainerContext = createContext<HTMLElement | null>(null);

/** The element floating surfaces are drawn into, `undefined` for the default (`<body>`). */
export function useFloatingContainer(): HTMLElement | undefined {
  return useContext(FloatingContainerContext) ?? undefined;
}
