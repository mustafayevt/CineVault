import { Fragment } from "react";

import { MenuItem } from "../../components/MenuItem";
import { MenuSeparator } from "../../components/Menu";
import { MENU_GROUPS } from "./titleActions";
import type { TitleActionSet } from "./useLibraryActions";

/**
 * The items of a title's right-click menu (spec library-browsing "Title right-click menu"), read
 * from the same action set as the selection bar: Open, Play, Open folder, then Rename, Scrape,
 * Artwork…, Move to…, Rewrite NFO and the rest, then the two deletes, with key hints and counts.
 * Pass it as the `menu` of a `Table` or a `ContextMenu`.
 */
export function TitleContextMenu({ actions }: { actions: TitleActionSet }) {
  return (
    <>
      {MENU_GROUPS.map((group, index) => (
        <Fragment key={group[0]}>
          {index > 0 && <MenuSeparator />}
          {group.map((id) => {
            const action = actions[id];
            return (
              <MenuItem
                key={id}
                icon={action.def.icon}
                shortcut={action.def.hint}
                danger={action.def.danger}
                disabledReason={action.disabledReason}
                onSelect={action.onSelect}
                data-action={id}
              >
                {action.label}
              </MenuItem>
            );
          })}
        </Fragment>
      ))}
    </>
  );
}
