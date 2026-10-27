import { useNavigate } from "@tanstack/react-router";
import { FileText } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useMemo } from "react";

import { t } from "../../i18n/catalog";
import { requestDelete, requestPermanentDelete } from "../delete/deleteActions";
import { filesStateOf, playBlockedFor } from "../detail/filesState";
import { useOpenFolder } from "../detail/useOpenFolder";
import { shortLabel, TITLE_ACTIONS } from "../library/titleActions";
import { runNfoAction, selectionScope } from "../nfo/nfoActions";
import { openMoveTo } from "../move-to/moveToStore";
import { playTitle } from "../playback/usePlay";
import { removeTitlesFromLibrary } from "./removeTitles";

/** The header buttons, in order: Play · Scrape · Rename · Move · Open folder · Delete. */
export type HeaderActionId = "play" | "scrape" | "rename" | "move" | "open-folder" | "delete";
/** What the ⋯ menu holds. */
export type MenuActionId =
  "edit" | "artwork" | "show-nfo" | "rewrite-nfo" | "remove" | "ignore" | "delete-permanently";

export interface TitleAction<Id extends string> {
  id: Id;
  label: string;
  icon: LucideIcon;
  onSelect: () => void;
  /** Why the action cannot run now; the control stays reachable and says so. */
  disabledReason?: string | undefined;
  danger?: boolean;
}

export interface TitleActions {
  header: TitleAction<HeaderActionId>[];
  menu: TitleAction<MenuActionId>[];
}

export interface TitleActionsInput {
  id: string;
  kind: "movie" | "show";
  title: string;
  folder: string;
  availability: string;
  /** What Play starts: the movie itself, or the show's next episode; `null` when there is none. */
  playId: string | null;
  /** Opens the NFO text in the page. */
  onShowNfo: () => void;
}

/**
 * The actions of a title page: the six header buttons and the ⋯ menu, built once so the Movie page
 * and the show page say the same things in the same order. Their icons, names and order come from
 * the one list in `features/library/titleActions.ts`, which the Library's selection bar and
 * right-click menu read too.  */
export function useTitleActions(input: TitleActionsInput): TitleActions {
  const { id, kind, title, folder, availability, playId, onShowNfo } = input;
  const navigate = useNavigate();
  const openFolder = useOpenFolder(title);

  return useMemo<TitleActions>(() => {
    const blocked = playBlockedFor(filesStateOf(availability), kind);
    // A deleted title has no page any more: go back to the Library.
    const leave = () => void navigate({ to: "/library" });
    const removeFromLibrary = (ignore: boolean) => {
      void removeTitlesFromLibrary([id], ignore, title).then((removed) => {
        if (removed) leave();
      });
    };
    return {
      header: [
        {
          id: "play",
          label: shortLabel("play"),
          icon: TITLE_ACTIONS["play"].icon,
          onSelect: () => {
            if (playId !== null) void playTitle(playId, title);
          },
          disabledReason: blocked ?? (playId === null ? t("title.play.nothing") : undefined),
        },
        {
          id: "scrape",
          label: shortLabel("scrape"),
          icon: TITLE_ACTIONS["scrape"].icon,
          onSelect: () => void navigate({ to: "/scrape/match/$itemId", params: { itemId: id } }),
        },
        {
          id: "rename",
          label: shortLabel("rename"),
          icon: TITLE_ACTIONS["rename"].icon,
          onSelect: () => void navigate({ to: "/rename", search: { ids: id } as never }),
        },
        {
          id: "move",
          label: shortLabel("move"),
          icon: TITLE_ACTIONS["move"].icon,
          // Opens the shared Move to menu under the button.
          onSelect: () => openMoveTo([id], kind, title),
        },
        {
          id: "open-folder",
          label: shortLabel("open-folder"),
          icon: TITLE_ACTIONS["open-folder"].icon,
          onSelect: () => openFolder(folder),
          disabledReason: folder === "" ? t("title.openFolder.none") : undefined,
        },
        {
          id: "delete",
          label: shortLabel("delete"),
          icon: TITLE_ACTIONS["delete"].icon,
          onSelect: () => requestDelete([{ id, title }], { onDone: leave }),
          danger: true,
        },
      ],
      menu: [
        {
          id: "edit",
          label: shortLabel("edit"),
          icon: TITLE_ACTIONS["edit"].icon,
          onSelect: () =>
            void (kind === "movie"
              ? navigate({ to: "/movies/$movieId/edit", params: { movieId: id } })
              : navigate({ to: "/tv/$showId/edit", params: { showId: id } })),
        },
        {
          id: "artwork",
          label: t(TITLE_ACTIONS["artwork"].full),
          icon: TITLE_ACTIONS["artwork"].icon,
          onSelect: () => void navigate({ to: "/artwork", search: { ids: id } as never }),
        },
        { id: "show-nfo", label: t("title.menu.showNfo"), icon: FileText, onSelect: onShowNfo },
        {
          id: "rewrite-nfo",
          label: shortLabel("rewrite-nfo"),
          icon: TITLE_ACTIONS["rewrite-nfo"].icon,
          onSelect: () => void runNfoAction("rewrite", selectionScope([id])),
        },
        {
          id: "remove",
          label: shortLabel("remove"),
          icon: TITLE_ACTIONS["remove"].icon,
          onSelect: () => removeFromLibrary(false),
        },
        {
          id: "ignore",
          label: shortLabel("ignore"),
          icon: TITLE_ACTIONS["ignore"].icon,
          onSelect: () => removeFromLibrary(true),
        },
        {
          id: "delete-permanently",
          label: shortLabel("delete-permanently"),
          icon: TITLE_ACTIONS["delete-permanently"].icon,
          onSelect: () => requestPermanentDelete([{ id, title }], { onDone: leave }),
          danger: true,
        },
      ],
    };
  }, [id, kind, title, folder, availability, playId, onShowNfo, navigate, openFolder]);
}
