import { useNavigate } from "@tanstack/react-router";
import { HardDrive, TriangleAlert } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { useEffect, useId, useRef, useState } from "react";
import type { CSSProperties } from "react";

import { Icon } from "../../components/Icon";
import { Menu } from "../../components/Menu";
import { PathText } from "../../components/PathText";
import { Skeleton } from "../../components/Skeleton";
import { SpaceBar } from "../../components/SpaceBar";
import { Tag } from "../../components/Tag";
import { menuItemClass } from "../../components/floating";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import type { MoveOption } from "./moveOptions";
import { moveToDestination } from "./runMove";
import { liveRect, trackPointer, useMoveToStore } from "./moveToStore";
import type { MoveAnchor, MoveKind } from "./moveToStore";
import { useMoveOptions } from "./useMoveOptions";

/**
 * A destination row: two lines and a third with the space after the move or the reason it cannot
 * be chosen. It overrides the one-line menu item (height, wrapping, alignment) with `!`.
 */
const rowClass = cx(menuItemClass, "h-auto! items-start! gap-2.5! py-2! whitespace-normal!");

interface DestinationRowProps {
  option: MoveOption;
  /** The titles by name, for the toast. */
  label: string;
  /** Why no row can be chosen right now (another move is running), else null. */
  busy: string | null;
}

/**
 * One destination: the drive's name and its free space, the library folder's path shortened in
 * the middle (the full path is its tooltip), and the space bar after the move. A destination that
 * cannot take the titles stays focusable, says why in the warning colour, and ignores choosing.
 */
function DestinationRow({ option, label, busy }: DestinationRowProps) {
  const format = useFormat();
  const reasonId = useId();
  const reason = option.reason ?? busy;
  const blocked = reason !== null;
  return (
    <DropdownMenu.Item
      data-destination={option.source.id}
      textValue={option.name}
      aria-disabled={blocked ? true : undefined}
      aria-describedby={blocked ? reasonId : undefined}
      onSelect={(event) => {
        if (blocked) {
          event.preventDefault();
          return;
        }
        void moveToDestination(option, label);
      }}
      className={cx(rowClass, blocked && "cursor-not-allowed")}
    >
      <Icon
        icon={HardDrive}
        className={cx("mt-0.5", blocked ? "text-text-disabled" : "text-text-muted")}
      />
      <span className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="flex min-w-0 items-baseline gap-2">
          <span
            className={cx(
              "min-w-0 shrink truncate font-(weight:--fw-600)",
              blocked ? "text-text-2" : "text-text",
            )}
          >
            {option.name}
          </span>
          {option.folder === null ? null : (
            <span className="min-w-0 shrink-[2] truncate text-(length:--fs-12) text-text-muted">
              {option.folder}
            </span>
          )}
          <span className="ml-auto shrink-0 pl-2 text-(length:--fs-12) text-text-2 tabular-nums">
            {option.offline ? (
              <Tag>{t("moveTo.offline")}</Tag>
            ) : option.freeNow === null ? null : (
              t("moveTo.freeNow", { free: format.size(option.freeNow) })
            )}
          </span>
        </span>
        <PathText
          path={option.path}
          className={cx(
            "font-mono text-(length:--fs-12)",
            blocked ? "text-text-disabled" : "text-text-muted",
          )}
        />
        {blocked ? (
          <span
            id={reasonId}
            data-reason=""
            className="flex items-start gap-1.5 text-(length:--fs-12) leading-snug text-warn"
          >
            <Icon icon={TriangleAlert} size={14} className="mt-px shrink-0" />
            <span>{reason}</span>
          </span>
        ) : option.usedAfter === null && option.freeAfter === null ? null : (
          <span className="flex items-center gap-2.5">
            {option.usedAfter === null ? null : (
              <SpaceBar
                used={option.usedAfter}
                label={t("moveTo.barLabel", { name: option.name })}
                size="md"
                className="min-w-0 flex-1"
              />
            )}
            {option.freeAfter === null ? null : (
              <span className="ml-auto shrink-0 text-(length:--fs-12) text-text-2 tabular-nums">
                {t("moveTo.afterHead")}{" "}
                <b className="font-(weight:--fw-600) text-text">{format.size(option.freeAfter)}</b>{" "}
                {t("moveTo.afterTail")}
              </span>
            )}
          </span>
        )}
      </span>
    </DropdownMenu.Item>
  );
}

interface MoveToItemsProps {
  ids: readonly string[];
  kind: MoveKind;
  /** The titles by name, for the toast ("Heat (1995)" or "4 titles"). */
  label: string;
  /** The menu was opened from the keyboard: the first destination that can be chosen takes focus. */
  keyboard: boolean;
}

/**
 * The inside of the Move to menu (specs `drives` "Move to with space before and after" and
 * `source-moves`): "Move 3 titles · 198.7 GB to", each other library folder of the same type with
 * its drive, free space, path, a used-space bar and the free space after the move, a destination
 * that cannot take the titles disabled with its reason, and what the source drive frees. Only the
 * list scrolls, and only when it is longer than the window allows. Choosing a destination starts
 * the move; nothing touches the disk before that.
 */
function MoveToItems({ ids, kind, label, keyboard }: MoveToItemsProps) {
  const format = useFormat();
  const navigate = useNavigate();
  const running = useMoveToStore((state) => state.running);
  const { loading, failed, retry, options, totalBytes, frees } = useMoveOptions(ids, kind);
  const list = useRef<HTMLDivElement>(null);
  const busy = running === null ? null : t("moveTo.busy");

  // From the keyboard, the first destination that can be chosen takes the focus once it is known.
  useEffect(() => {
    if (!keyboard || loading) return;
    const first =
      list.current?.querySelector<HTMLElement>('[data-destination]:not([aria-disabled="true"])') ??
      list.current?.querySelector<HTMLElement>('[role="menuitem"]');
    first?.focus();
  }, [keyboard, loading]);

  return (
    <>
      <DropdownMenu.Label className="shrink-0 px-3 pt-2.5 pb-1.5 text-(length:--fs-12) font-(weight:--fw-600) text-text-2">
        {totalBytes === null
          ? t("moveTo.label.count", { count: ids.length })
          : t("moveTo.label", { count: ids.length, size: format.size(totalBytes) })}
      </DropdownMenu.Label>
      <div
        ref={list}
        aria-busy={loading || undefined}
        className="flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto px-1 pb-1"
      >
        {loading ? (
          <>
            <p role="status" className="sr-only">
              {t("moveTo.reading")}
            </p>
            {[0, 1].map((row) => (
              <div key={row} aria-hidden="true" className="flex flex-col gap-1.5 px-2.5 py-2">
                <Skeleton shape="line" width="55%" />
                <Skeleton shape="line" width="80%" />
              </div>
            ))}
          </>
        ) : failed ? (
          <div className="flex flex-col gap-1 px-2.5 py-2">
            <p role="alert" className="m-0 text-(length:--fs-12) text-text-2">
              {t("moveTo.error")}
            </p>
            <DropdownMenu.Item
              onSelect={(event) => {
                event.preventDefault();
                retry();
              }}
              className={cx(menuItemClass, "-mx-2.5 text-gold")}
            >
              {t("moveTo.retry")}
            </DropdownMenu.Item>
          </div>
        ) : options.length === 0 ? (
          <div className="flex flex-col gap-1 px-2.5 py-2">
            <p className="m-0 text-(length:--fs-13) text-text">
              {t(kind === "movie" ? "moveTo.none.movie" : "moveTo.none.tv")}
            </p>
            <p className="m-0 text-(length:--fs-12) text-text-2">{t("moveTo.none.hint")}</p>
            <DropdownMenu.Item
              onSelect={() => void navigate({ to: "/settings", search: { section: "folders" } })}
              className={cx(menuItemClass, "-mx-2.5 mt-1 text-gold")}
            >
              {t("moveTo.none.action")}
            </DropdownMenu.Item>
          </div>
        ) : (
          options.map((option) => (
            <DestinationRow key={option.source.id} option={option} label={label} busy={busy} />
          ))
        )}
      </div>
      {frees === null || loading ? null : (
        <p className="m-0 shrink-0 border-t border-solid border-line-section px-3 py-2 text-(length:--fs-12) text-text-2">
          {t("moveTo.sourceHead", { name: frees.name, before: format.size(frees.before) })}{" "}
          <b className="font-(weight:--fw-600) text-text">{format.size(frees.after)}</b>{" "}
          {t("moveTo.sourceTail")}
        </p>
      )}
    </>
  );
}

type Rect = Pick<MoveAnchor, "x" | "y" | "width" | "height">;

/**
 * Where the anchor's control is, followed every frame while the menu is open (the selection bar
 * slides in after the click that opened the menu). Calls `onGone` when the control leaves the page.
 */
function useAnchorRect(anchor: MoveAnchor | null, onGone: () => void): Rect | null {
  const [seen, setSeen] = useState<{ anchor: MoveAnchor; rect: Rect } | null>(null);
  const gone = useRef(onGone);
  useEffect(() => {
    gone.current = onGone;
  });
  useEffect(() => {
    if (anchor === null) return;
    let frame = 0;
    const tick = () => {
      const element = anchor.element;
      if (element !== undefined && element !== null && !element.isConnected) {
        // The control went away (the selection was cleared): the menu goes with it.
        gone.current();
        return;
      }
      const rect = liveRect(anchor);
      setSeen((last) =>
        last?.anchor === anchor &&
        last.rect.x === rect.x &&
        last.rect.y === rect.y &&
        last.rect.width === rect.width &&
        last.rect.height === rect.height
          ? last
          : { anchor, rect },
      );
      frame = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(frame);
  }, [anchor]);
  if (anchor === null) return null;
  return seen?.anchor === anchor ? seen.rect : liveRect(anchor);
}

/** At most this tall, and never taller than the room the window leaves beside the anchor. */
const surfaceStyle: CSSProperties = {
  maxHeight: "min(var(--radix-dropdown-menu-content-available-height), 440px)",
};

/**
 * Opens the Move to menu for a request made elsewhere (a right-click menu item, a title page
 * button, the selection bar of the Library and Drives): the menu appears beside the control that
 * asked and follows it, or opens at the pointer. Mounted once in the app shell. It is not modal: a
 * press outside closes it and still reaches what was pressed, and pressing the control that
 * opened it closes it again.
 */
export function MoveToHost() {
  const request = useMoveToStore((state) => state.request);
  const close = useMoveToStore((state) => state.close);
  const anchor = request?.anchor ?? null;
  const rect = useAnchorRect(anchor, close);
  const pressedOutside = useRef(false);

  useEffect(() => {
    window.addEventListener("pointerdown", trackPointer, true);
    return () => window.removeEventListener("pointerdown", trackPointer, true);
  }, []);

  if (request === null || anchor === null || rect === null) return null;
  const opener = anchor.element ?? null;
  return (
    <Menu
      // A new request is a new menu: its focus and highlight start over.
      key={request.id}
      open
      modal={false}
      onOpenChange={(open) => {
        if (!open) close();
      }}
      side={anchor.side}
      align="start"
      aria-label={t("moveTo.menu")}
      className="flex! w-[min(372px,calc(100vw-16px))] flex-col overflow-hidden! p-0!"
      style={surfaceStyle}
      onOpenAutoFocus={() => {
        pressedOutside.current = false;
      }}
      onPointerDownOutside={(event) => {
        // The control that opened the menu toggles it on its click; closing here first would
        // open it again on that click.
        const target = event.detail.originalEvent.target;
        if (opener !== null && target instanceof Node && opener.contains(target)) {
          event.preventDefault();
          return;
        }
        pressedOutside.current = true;
      }}
      // Focus moving elsewhere by itself (a closing right-click menu hands it back to the list)
      // must not close the menu that menu just opened; a press outside or Esc does.
      onFocusOutside={(event) => event.preventDefault()}
      onCloseAutoFocus={(event) => {
        event.preventDefault();
        // Back to the control that opened the menu, unless the user pressed somewhere else.
        if (!pressedOutside.current && opener?.isConnected === true) opener.focus();
      }}
      trigger={
        <span
          aria-hidden="true"
          style={{
            position: "fixed",
            left: rect.x,
            top: rect.y,
            width: rect.width,
            height: rect.height,
            pointerEvents: "none",
          }}
        />
      }
    >
      <MoveToItems
        ids={request.ids}
        kind={request.kind}
        label={request.label}
        keyboard={request.keyboard}
      />
    </Menu>
  );
}
