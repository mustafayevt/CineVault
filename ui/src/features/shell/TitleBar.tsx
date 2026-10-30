import { useNavigate, useRouterState } from "@tanstack/react-router";
import { useState } from "react";

import { SpotlightSearch } from "../search/Spotlight";
import { transport } from "../../lib/transport";
import { CaptionButtons } from "./CaptionButtons";
import { LIBRARY_SEARCH_ATTRIBUTE } from "./desktopGuards";
import type { Platform } from "./platform";
import { useWindowMaximized } from "./windowApi";
import type { WindowApi } from "./windowApi";

interface TitleBarProps {
  platform: Platform;
  /** The window to control; `null` in a plain browser. */
  windowApi: WindowApi | null;
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
 */
export function TitleBar({ platform, windowApi }: TitleBarProps) {
  const maximized = useWindowMaximized(windowApi);
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
        {transport() === "desktop" && (platform === "windows" || platform === "linux") ? (
          <CaptionButtons windowApi={windowApi} maximized={maximized} />
        ) : null}
      </div>
    </header>
  );
}
