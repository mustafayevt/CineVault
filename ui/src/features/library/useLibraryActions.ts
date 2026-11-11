import { useNavigate } from "@tanstack/react-router";
import { useMemo } from "react";

import { t } from "../../i18n/catalog";
import { filesStateOf, playBlockedFor } from "../detail/filesState";
import { openMoveTo } from "../move-to/moveToStore";
import { useOpenFolder } from "../detail/useOpenFolder";
import { reloadMediaInfo } from "../mediainfo/reloadMediaInfo";
import { runNfoAction, selectionScope } from "../nfo/nfoActions";
import { playTitle } from "../playback/usePlay";
import { requestDelete } from "../delete/deleteActions";
import { useMetadataOnly } from "../settings/metadataOnly";
import { removeTitlesFromLibrary } from "../title/removeTitles";
import { setTitleChoice } from "../title/titleChoice";
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
  /** The copies of the movie in its group, kept or not; 0 or absent when it has none. */
  copies?: number | undefined;
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
  /** The menu leaves it out (Compare for a title without copies). */
  hidden?: boolean;
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
  const metadataOnly = useMetadataOnly();

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
      "compare-copies": {
        onSelect: () => {
          if (single !== undefined)
            void navigate({ to: "/movies/$movieId/copies", params: { movieId: single.id } });
        },
        disabledReason: needOne,
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
      "use-original-title": {
        onSelect: () => void setTitleChoice(ids, true, subjectOf(targets)),
        disabledReason: none,
      },
      "use-localized-title": {
        onSelect: () => void setTitleChoice(ids, false, subjectOf(targets)),
        disabledReason: none,
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
    };

    const copies = single?.kind === "movie" ? (single.copies ?? 0) : 0;
    const entries = IDS.map((id): [TitleActionId, ResolvedTitleAction] => {
      const def = TITLE_ACTIONS[id];
      // Compare is named for the copies of the film, not for the selection.
      const compare = id === "compare-copies";
      return [
        id,
        {
          def,
          id,
          short: compare ? t("copies.act.compare", { count: copies }) : shortLabel(id),
          label: compare ? t("copies.act.compare", { count: copies }) : fullLabel(id, count),
          ...run[id],
          ...((compare && copies === 0) ||
          // "Metadata only" never renames or moves files, so neither is offered.
          (metadataOnly && (id === "rename" || id === "move"))
            ? { hidden: true }
            : {}),
        },
      ];
    });
    return Object.fromEntries(entries) as TitleActionSet;
  }, [targets, first, navigate, openFolder, metadataOnly]);
}
