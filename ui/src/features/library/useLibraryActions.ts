import { useNavigate } from "@tanstack/react-router";
import { useMemo } from "react";

import { t } from "../../i18n/catalog";
import { filesStateOf, playBlockedFor } from "../detail/filesState";
import { openMoveTo } from "../move-to/moveToStore";
import { useOpenFolder } from "../detail/useOpenFolder";
import { reloadMediaInfo } from "../mediainfo/reloadMediaInfo";
import { runNfoAction, selectionScope } from "../nfo/nfoActions";
import { playTitle } from "../playback/usePlay";
import { requestDelete, requestPermanentDelete } from "../delete/deleteActions";
import { removeTitlesFromLibrary } from "../title/removeTitles";
import { fullLabel, shortLabel, TITLE_ACTIONS } from "./titleActions";
import type { TitleActionDef, TitleActionId } from "./titleActions";

/** What an action needs to know about a selected title. */
export interface TitleTarget {
  id: string;
  kind: "movie" | "show";
  title: string;
  /** The title's folder; empty when unknown. */
  folder: string;
  /** The files cannot be reached (the Missing flag: missing or offline). */
  filesMissing: boolean;
}

/** One action ready to render in the selection bar, the right-click menu or More. */
export interface ResolvedTitleAction {
  def: TitleActionDef;
  id: TitleActionId;
  /** The button name. */
  short: string;
  /** The menu name, with the count when several titles are selected. */
  label: string;
  onSelect: () => void;
  /** Why it cannot run now; the control stays reachable and says so. */
  disabledReason?: string | undefined;
}

export type TitleActionSet = Readonly<Record<TitleActionId, ResolvedTitleAction>>;

const IDS = Object.keys(TITLE_ACTIONS) as TitleActionId[];

/** "Heat" for one title, "4 titles" for several. */
function subjectOf(targets: readonly TitleTarget[]): string {
  const [only] = targets;
  return targets.length === 1 && only !== undefined
    ? only.title
    : t("library.act.subject", { count: targets.length });
}

/** The ids and names Delete works with. */
function namesOf(targets: readonly TitleTarget[]): { id: string; title: string }[] {
  return targets.map((target) => ({ id: target.id, title: target.title }));
}

/**
 * The actions of the selected titles: the single set the selection bar, the right-click
 * menu and More all render, so their names and order cannot drift. Actions that work on one title
 * say "Select one title" when several are selected.
 */
export function useLibraryActions(targets: readonly TitleTarget[]): TitleActionSet {
  const navigate = useNavigate();
  const [first] = targets;
  const openFolder = useOpenFolder(first?.title ?? "");

  return useMemo<TitleActionSet>(() => {
    const ids = targets.map((target) => target.id);
    const count = targets.length;
    const single = count === 1 ? first : undefined;
    const needOne = count === 1 ? undefined : t("library.act.needOne");
    const none = count === 0 ? t("library.act.needSome") : undefined;

    const run: Record<
      TitleActionId,
      { onSelect: () => void; disabledReason?: string | undefined }
    > = {
      open: {
        onSelect: () => {
          if (single === undefined) return;
          if (single.kind === "movie")
            void navigate({ to: "/movies/$movieId", params: { movieId: single.id } });
          else void navigate({ to: "/tv/$showId", params: { showId: single.id } });
        },
        disabledReason: needOne,
      },
      play: {
        onSelect: () => {
          if (single !== undefined) void playTitle(single.id, single.title);
        },
        disabledReason:
          needOne ??
          (single?.kind === "show"
            ? t("library.act.playShow")
            : playBlockedFor(
                filesStateOf(single?.filesMissing === true ? "missing" : ""),
                "movie",
              )),
      },
      "open-folder": {
        onSelect: () => {
          if (single !== undefined) openFolder(single.folder);
        },
        disabledReason: needOne ?? (single?.folder === "" ? t("title.openFolder.none") : undefined),
      },
      rename: {
        onSelect: () => void navigate({ to: "/rename", search: { ids: ids.join(",") } as never }),
        disabledReason: none,
      },
      scrape: {
        onSelect: () => {
          if (single !== undefined) {
            void navigate({ to: "/scrape/match/$itemId", params: { itemId: single.id } });
          } else {
            void navigate({ to: "/scrape", search: { ids: ids.join(",") } as never });
          }
        },
        disabledReason: none,
      },
      artwork: {
        onSelect: () => void navigate({ to: "/artwork", search: { ids: ids.join(",") } as never }),
        disabledReason: none,
      },
      move: {
        // Opens the shared Move to menu beside the control that asked.
        onSelect: () => openMoveTo(ids, first?.kind ?? "movie", subjectOf(targets)),
        disabledReason: none,
      },
      "rewrite-nfo": {
        onSelect: () => void runNfoAction("rewrite", selectionScope(ids)),
        disabledReason: none,
      },
      "reload-media-info": {
        onSelect: () => void reloadMediaInfo(ids),
        disabledReason: none,
      },
      edit: {
        onSelect: () => {
          if (single === undefined) return;
          if (single.kind === "movie")
            void navigate({ to: "/movies/$movieId/edit", params: { movieId: single.id } });
          else void navigate({ to: "/tv/$showId/edit", params: { showId: single.id } });
        },
        disabledReason: needOne,
      },
      remove: {
        onSelect: () => void removeTitlesFromLibrary(ids, false, subjectOf(targets)),
        disabledReason: none,
      },
      ignore: {
        onSelect: () => void removeTitlesFromLibrary(ids, true, subjectOf(targets)),
        disabledReason: none,
      },
      delete: {
        onSelect: () => requestDelete(namesOf(targets)),
        disabledReason: none,
      },
      "delete-permanently": {
        onSelect: () => requestPermanentDelete(namesOf(targets)),
        disabledReason: none,
      },
    };

    const entries = IDS.map((id): [TitleActionId, ResolvedTitleAction] => {
      const def = TITLE_ACTIONS[id];
      return [
        id,
        {
          def,
          id,
          short: shortLabel(id),
          label: fullLabel(id, count),
          ...run[id],
        },
      ];
    });
    return Object.fromEntries(entries) as TitleActionSet;
  }, [targets, first, navigate, openFolder]);
}
