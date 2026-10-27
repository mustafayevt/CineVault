import { useEffect } from "react";

import { Menu, MenuLabel, MenuSeparator } from "../../components/Menu";
import { MenuItem } from "../../components/MenuItem";
import { SpaceBar } from "../../components/SpaceBar";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { moveToDestination } from "./runMove";
import { trackPointer, useMoveToStore } from "./moveToStore";
import type { MoveKind } from "./moveToStore";
import { useMoveOptions } from "./useMoveOptions";

interface MoveToItemsProps {
  ids: readonly string[];
  kind: MoveKind;
  /** The titles by name, for the toast ("Heat (1995)" or "4 titles"). */
  label: string;
}

/**
 * The inside of the Move to menu (specs `drives` "Move to with space before and
 * after" and `source-moves`): "Move 3 titles · 198.7 GB to", each other library folder of the same
 * type with its drive, a used-space bar and "5.81 TB free → 5.61 TB after", a destination that
 * cannot take the titles disabled with its reason, and what the source drive frees. Choosing a
 * destination starts the move; nothing touches the disk before that.
 */
function MoveToItems({ ids, kind, label }: MoveToItemsProps) {
  const format = useFormat();
  const { loading, options, totalBytes, frees } = useMoveOptions(ids, kind);
  return (
    <>
      <MenuLabel>
        {totalBytes === null
          ? t("moveTo.label.count", { count: ids.length })
          : t("moveTo.label", { count: ids.length, size: format.size(totalBytes) })}
      </MenuLabel>
      {loading ? (
        <p role="status" className="m-0 px-2.5 py-2 text-(length:--fs-12) text-text-muted">
          {t("moveTo.reading")}
        </p>
      ) : options.length === 0 ? (
        <p className="m-0 px-2.5 py-2 text-(length:--fs-12) text-text-2">
          {t(kind === "movie" ? "moveTo.none.movie" : "moveTo.none.tv")}
        </p>
      ) : (
        options.map((option) => (
          <MenuItem
            key={option.source.id}
            {...(option.reason === null ? {} : { disabledReason: option.reason })}
            onSelect={() => void moveToDestination(option, label)}
            data-destination={option.source.id}
            className="h-auto! flex-col! items-stretch! gap-1.5! px-2.5! py-2!"
          >
            <span className="flex items-baseline gap-2">
              <span className="font-(weight:--fw-600)">{option.name}</span>
              {option.offline ? (
                <Tag>{t("moveTo.offline")}</Tag>
              ) : (
                <span className="cv-selectable min-w-0 truncate font-mono text-(length:--fs-12) text-text-muted">
                  {option.path}
                </span>
              )}
            </span>
            {option.reason !== null ? (
              option.offline ? null : (
                <span className="text-(length:--fs-12) whitespace-normal text-text-muted">
                  {option.reason}
                </span>
              )
            ) : (
              <>
                {option.usedAfter === null ? null : (
                  <SpaceBar
                    used={option.usedAfter}
                    label={t("moveTo.barLabel", { name: option.name })}
                    size="md"
                  />
                )}
                {option.freeNow === null || option.freeAfter === null ? null : (
                  <span className="text-(length:--fs-12) text-text-2">
                    {t("moveTo.free", { free: format.size(option.freeNow) })}{" "}
                    <b className="font-(weight:--fw-600) text-text">
                      {format.size(option.freeAfter)}
                    </b>{" "}
                    {t("moveTo.after")}
                  </span>
                )}
              </>
            )}
          </MenuItem>
        ))
      )}
      {frees === null ? null : (
        <>
          <MenuSeparator />
          <p className="m-0 px-2.5 py-1.5 text-(length:--fs-12) text-text-2">
            {t("moveTo.sourceHead", { name: frees.name, before: format.size(frees.before) })}{" "}
            <b className="font-(weight:--fw-600) text-text">{format.size(frees.after)}</b>{" "}
            {t("moveTo.sourceTail")}
          </p>
        </>
      )}
    </>
  );
}

/**
 * Opens the Move to menu for a request made elsewhere (a right-click menu item, a title page
 * button): the menu appears beside the control that asked, or at the pointer. Mounted once in the
 * app shell; the selection bar of the Library and Drives asks through `openMoveTo` too.
 */
export function MoveToHost() {
  const request = useMoveToStore((state) => state.request);
  const close = useMoveToStore((state) => state.close);

  useEffect(() => {
    window.addEventListener("pointerdown", trackPointer, true);
    return () => window.removeEventListener("pointerdown", trackPointer, true);
  }, []);

  if (request === null) return null;
  const { anchor } = request;
  return (
    <Menu
      open
      onOpenChange={(open) => {
        if (!open) close();
      }}
      side={anchor.side}
      align="start"
      aria-label={t("moveTo.menu")}
      className={cx("w-[320px]")}
      trigger={
        <span
          aria-hidden="true"
          style={{
            position: "fixed",
            left: anchor.x,
            top: anchor.y,
            width: anchor.width,
            height: anchor.height,
            pointerEvents: "none",
          }}
        />
      }
    >
      <MoveToItems ids={request.ids} kind={request.kind} label={request.label} />
    </Menu>
  );
}
