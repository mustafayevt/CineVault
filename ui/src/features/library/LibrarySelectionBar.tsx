import { SelectionBar } from "../../components/SelectionBar";
import type { SelectionBarAction } from "../../components/SelectionBar";
import { t } from "../../i18n/catalog";
import { BAR_ORDER, MORE_ORDER } from "./titleActions";
import type { TitleActionId } from "./titleActions";
import type { TitleActionSet } from "./useLibraryActions";

interface LibrarySelectionBarProps {
  actions: TitleActionSet;
  count: number;
  /** The total size of the selection, formatted ("186.2 GB"); left out while it is unknown. */
  size?: string | undefined;
  onClear: () => void;
  /**
   * Shows "Remove from library" as a button of its own, before Delete, instead of under More. The
   * Library turns it on in the "Missing files or offline" view, where taking titles out whose
   * files are gone is the likely next step.
   */
  promoteRemove?: boolean;
}

/**
 * The floating selection bar of the Library (spec library-browsing "Click selects and the floating
 * selection bar"): the count and total size, then Rename, Scrape, Artwork, Move, Delete and More,
 * all read from the one title-actions list so the bar, the right-click menu and the title pages
 * say the same things in the same order.
 */
export function LibrarySelectionBar({
  actions,
  count,
  size,
  onClear,
  promoteRemove = false,
}: LibrarySelectionBarProps) {
  const toBar = (id: TitleActionId, label: "short" | "label"): SelectionBarAction => {
    const action = actions[id];
    return {
      id,
      // "Remove from library (keep files)" is long for a button; its menu name stays complete.
      label: id === "remove" && label === "short" ? t("library.act.removeShort") : action[label],
      icon: action.def.icon,
      onSelect: action.onSelect,
      ...(action.def.hint === undefined ? {} : { shortcut: action.def.hint }),
      ...(action.def.danger === true ? { danger: true } : {}),
      ...(action.disabledReason === undefined ? {} : { disabledReason: action.disabledReason }),
    };
  };
  const barIds: readonly TitleActionId[] = promoteRemove
    ? BAR_ORDER.flatMap((id) => (id === "delete" ? ["remove" as const, id] : [id]))
    : BAR_ORDER;
  const moreIds = promoteRemove ? MORE_ORDER.filter((id) => id !== "remove") : MORE_ORDER;
  return (
    <SelectionBar
      count={count}
      {...(size === undefined ? {} : { size })}
      actions={barIds.filter((id) => actions[id].hidden !== true).map((id) => toBar(id, "short"))}
      more={moreIds.filter((id) => actions[id].hidden !== true).map((id) => toBar(id, "label"))}
      onClear={onClear}
    />
  );
}
