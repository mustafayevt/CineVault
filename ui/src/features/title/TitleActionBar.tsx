import { Ellipsis } from "lucide-react";
import { Fragment } from "react";

import { Button } from "../../components/Button";
import { IconButton } from "../../components/IconButton";
import { Menu, MenuSeparator } from "../../components/Menu";
import { MenuItem } from "../../components/MenuItem";
import { t } from "../../i18n/catalog";
import type { TitleActions } from "./titleActions";

/**
 * The action row of a title page: Play (the one gold button) · Scrape · Rename · Move · Open
 * folder · Change artwork, then the ⋯ menu, which ends with Delete… in the danger colour.
 */
export function TitleActionBar({ actions, label }: { actions: TitleActions; label: string }) {
  return (
    <div role="toolbar" aria-label={label} className="flex items-center gap-2">
      {actions.header.map((action) => (
        <Button
          key={action.id}
          variant={action.id === "play" ? "primary" : "default"}
          icon={action.icon}
          onClick={action.onSelect}
          disabledReason={action.disabledReason}
          data-action={action.id}
        >
          {action.label}
        </Button>
      ))}
      <Menu
        align="end"
        aria-label={t("title.menu.label")}
        trigger={<IconButton icon={Ellipsis} variant="default" aria-label={t("title.menu.more")} />}
      >
        {actions.menu.map((entry) => (
          <Fragment key={entry.id}>
            {entry.id === "remove" || entry.id === "delete-permanently" ? <MenuSeparator /> : null}
            <MenuItem
              icon={entry.icon}
              danger={entry.danger}
              onSelect={entry.onSelect}
              data-action={entry.id}
            >
              {entry.label}
            </MenuItem>
          </Fragment>
        ))}
      </Menu>
    </div>
  );
}
