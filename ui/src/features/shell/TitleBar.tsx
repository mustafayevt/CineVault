import { useNavigate, useRouterState } from "@tanstack/react-router";
import { ChevronLeft, Menu } from "lucide-react";
import { useState } from "react";

import { IconButton } from "../../components/IconButton";
import { t } from "../../i18n/catalog";
import { useShellStore } from "../../stores/shell";
import { SpotlightSearch } from "../search/Spotlight";
import { isServer } from "../../lib/transport";
import { useBackForwardInput } from "./backForward";
import { CaptionButtons } from "./CaptionButtons";
import { LIBRARY_SEARCH_ATTRIBUTE } from "./desktopGuards";
import { NAV_MENU_ATTRIBUTE } from "./NavDrawer";
import type { Platform } from "./platform";
import { useNavigationHistory } from "./useNavigationHistory";
import { useWindowMaximized } from "./windowApi";
import type { WindowApi } from "./windowApi";

interface TitleBarProps {
  platform: Platform;
  /** The window to control; `null` in a plain browser. */
  windowApi: WindowApi | null;
  /** The compact layout: the menu button and Back at the left, the search filling the rest. */
  compact?: boolean;
}

/** The `q` of the current address, when the page is the Library. */
function libraryQuery(pathname: string, search: unknown): string {
  if (pathname !== "/library") return "";
  const q = (search as { q?: unknown } | undefined)?.q;
  if (typeof q === "string") return q;
  return typeof q === "number" ? String(q) : "";
}

/**
 * The title bar over the content column: 38px on the
 * window ground with no line under it. The bar is a drag region (a drag moves the window, a
 * double-click maximises or restores it); the library search sits in its centre, 440px wide, in
 * the card colour, with the Ctrl F hint; on Windows and Linux the caption buttons sit flush at the
 * top right, 46 by 32. On macOS the traffic lights are in the sidebar panel's top row, so the bar
 * holds only the search. The search suggests titles as you type (see `SpotlightSearch`); Enter
 * with no suggestion highlighted opens the Library with `?q=`.
 *
 * In the compact layout of a phone or tablet (spec `app-shell` "Phone and tablet layout") the bar
 * is 52px: the menu button that opens the navigation drawer and Back sit at the left, and the
 * search fills the rest without its shortcut hint.
 */
export function TitleBar({ platform, windowApi, compact = false }: TitleBarProps) {
  const maximized = useWindowMaximized(windowApi);
  const nav = useNavigationHistory();
  useBackForwardInput(compact);
  const drawerOpen = useShellStore((state) => state.navSheetOpen);
  const openDrawer = useShellStore((state) => state.setNavSheetOpen);
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const urlQuery = useRouterState({
    select: (state) => libraryQuery(state.location.pathname, state.location.search),
  });
  const [text, setText] = useState(urlQuery);
  const [seen, setSeen] = useState(urlQuery);
  // The field follows the address: a link to `/library?q=heat` shows "heat" here.
  if (seen !== urlQuery) {
    setSeen(urlQuery);
    setText(urlQuery);
  }

  const open = (query: string, type?: "collections") => {
    const q = query.trim();
    void navigate({
      to: "/library",
      // On the Library the other choices (type, view, filters) stay; elsewhere it starts fresh.
      search: ((previous: Record<string, unknown>) => ({
        ...(pathname === "/library" ? previous : {}),
        ...(type === undefined ? {} : { type }),
        q: q === "" ? undefined : q,
      })) as never,
    });
  };

  if (compact) {
    return (
      <header
        data-testid="title-row"
        data-compact="true"
        className="flex h-[52px] shrink-0 items-center gap-1 pr-(--gutter) pl-1 select-none"
      >
        <IconButton
          icon={Menu}
          size="lg"
          aria-label={t("shell.menu")}
          aria-expanded={drawerOpen}
          aria-haspopup="dialog"
          {...{ [NAV_MENU_ATTRIBUTE]: "" }}
          onClick={() => openDrawer(true)}
        />
        <IconButton
          icon={ChevronLeft}
          size="lg"
          aria-label={t("chrome.back")}
          onClick={nav.back}
          disabled={!nav.canGoBack}
        />
        <SpotlightSearch
          compact
          text={text}
          setText={setText}
          shortcut=""
          fieldAttributes={{ [LIBRARY_SEARCH_ATTRIBUTE]: "" }}
          openLibrary={open}
          onClear={() => {
            setText("");
            if (urlQuery !== "") open("");
          }}
        />
      </header>
    );
  }

  return (
    <header
      data-tauri-drag-region
      data-testid="title-row"
      className="flex h-[38px] shrink-0 items-start bg-transparent select-none"
    >
      <div data-tauri-drag-region className="h-[38px] min-w-0 flex-1 basis-0" />
      <SpotlightSearch
        text={text}
        setText={setText}
        shortcut={platform === "macos" ? "⌘ F" : "Ctrl F"}
        fieldAttributes={{ [LIBRARY_SEARCH_ATTRIBUTE]: "" }}
        openLibrary={open}
        onClear={() => {
          setText("");
          if (urlQuery !== "") open("");
        }}
      />
      <div
        data-tauri-drag-region
        className="flex min-w-0 flex-1 basis-0 items-start justify-end self-stretch"
      >
        {!isServer() && (platform === "windows" || platform === "linux") ? (
          <CaptionButtons windowApi={windowApi} maximized={maximized} />
        ) : null}
      </div>
    </header>
  );
}
