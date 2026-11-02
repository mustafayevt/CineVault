import { useRouterState } from "@tanstack/react-router";
import { Dialog as RadixDialog } from "radix-ui";
import { useEffect, useRef } from "react";

import { useFloatingContainer } from "../../components/floating";
import { t } from "../../i18n/catalog";
import { useShellStore } from "../../stores/shell";
import type { Platform } from "./platform";
import { SidebarPanel } from "./SidebarPanel";

/** Marks the title bar's menu button, which opens the drawer and takes focus back after. */
export const NAV_MENU_ATTRIBUTE = "data-nav-menu";

interface NavDrawerProps {
  platform: Platform;
}

/**
 * The sidebar of the compact layout (spec `app-shell` "Phone and tablet layout"): the same panel,
 * full height at the left over a scrim, opened by the menu button in the title bar. Choosing a
 * page, a tap on the scrim or Esc closes it; focus stays inside while it is open and goes back to
 * the menu button after.
 */
export function NavDrawer({ platform }: NavDrawerProps) {
  const open = useShellStore((state) => state.navSheetOpen);
  const setOpen = useShellStore((state) => state.setNavSheetOpen);
  const container = useFloatingContainer();
  const location = useRouterState({
    select: (state) => state.location.pathname + state.location.searchStr,
  });
  // A link chosen in the drawer changes the address: the drawer gets out of the way.
  const seen = useRef(location);
  useEffect(() => {
    if (seen.current === location) return;
    seen.current = location;
    setOpen(false);
  }, [location, setOpen]);
  return (
    <RadixDialog.Root open={open} onOpenChange={setOpen}>
      <RadixDialog.Portal container={container}>
        <RadixDialog.Overlay className="cv-xfade fixed inset-0 z-50 bg-scrim" />
        <RadixDialog.Content
          aria-describedby={undefined}
          data-testid="nav-drawer"
          onOpenAutoFocus={(event) => {
            // Focus starts on the page being shown, not on the first control of the panel.
            const current = (event.currentTarget as HTMLElement | null)?.querySelector<HTMLElement>(
              '[aria-current="page"]',
            );
            if (current === null || current === undefined) return;
            event.preventDefault();
            current.focus();
          }}
          onCloseAutoFocus={(event) => {
            // Back to the menu button that opened it.
            const menu = document.querySelector<HTMLElement>(`[${NAV_MENU_ATTRIBUTE}]`);
            if (menu === null) return;
            event.preventDefault();
            menu.focus();
          }}
          className="cv-slide-up fixed inset-y-0 left-0 z-50 flex max-w-[calc(100vw-48px)] p-(--gutter) pt-[max(var(--gutter),env(safe-area-inset-top))] pb-[max(var(--gutter),env(safe-area-inset-bottom))] pl-[max(var(--gutter),env(safe-area-inset-left))] outline-none [--cv-slide-x:-24px] [--cv-slide-y:0px]"
        >
          <RadixDialog.Title className="sr-only">{t("shell.sidebar")}</RadixDialog.Title>
          <SidebarPanel platform={platform} variant="drawer" />
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}
