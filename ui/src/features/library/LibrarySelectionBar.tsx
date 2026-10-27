import { SelectionBar } from "../../components/SelectionBar";
import type { SelectionBarAction } from "../../components/SelectionBar";
import { BAR_ORDER, MORE_ORDER } from "./titleActions";
import type { TitleActionSet } from "./useLibraryActions";

interface LibrarySelectionBarProps {
  actions: TitleActionSet;
  count: number;
  /** The total size of the selection, formatted ("186.2 GB"); left out while it is unknown. */
  size?: string | undefined;
  onClear: () => void;
}

/**
 * The floating selection bar of the Library (spec library-browsing "Click selects and the floating
 * selection bar"): the count and total size, then Rename, Scrape, Artwork, Move, Delete and More,
 * all read from the one title-actions list so the bar, the right-click menu and the title pages
 * say the same things in the same order.
 */
export function LibrarySelectionBar({ actions, count, size, onClear }: LibrarySelectionBarProps) {
  const toBar = (id: (typeof BAR_ORDER)[number], label: "short" | "label"): SelectionBarAction => {
    const action = actions[id];
    return {
      id,
      label: action[label],
      icon: action.def.icon,
      onSelect: action.onSelect,
      ...(action.def.hint === undefined ? {} : { shortcut: action.def.hint }),
      ...(action.def.danger === true ? { danger: true } : {}),
      ...(action.disabledReason === undefined ? {} : { disabledReason: action.disabledReason }),
    };
  };
  return (
    <SelectionBar
      count={count}
      {...(size === undefined ? {} : { size })}
      actions={BAR_ORDER.map((id) => toBar(id, "short"))}
      more={MORE_ORDER.map((id) => toBar(id, "label"))}
      onClear={onClear}
    />
  );
}
