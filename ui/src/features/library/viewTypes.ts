import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";

import type { ListSelection } from "../../components/useListSelection";
import type { TitleType } from "./libraryStore";
import type { Listing } from "./useLibraryListing";

/** What the grid and the table of the Library share. */
export interface ListViewProps {
  type: TitleType;
  listing: Listing;
  selection: ListSelection<string>;
  /** The items of the right-click menu, the same actions as the selection bar. */
  menu: ReactNode;
  /** Room under the last row: 16, plus the selection bar's height while it is shown. */
  bottomRoom: number;
  /** Shown instead of the rows when nothing matches. */
  empty: ReactNode;
  /** F2, Delete and Shift+Delete; true when the key did something. */
  onKey: (event: ReactKeyboardEvent | KeyboardEvent) => boolean;
}
