import { useEffect, useId, useLayoutEffect, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import type { LucideIcon } from "lucide-react";

import type { PosterDto } from "../../bindings";
import { floatingSurface, useFloatingContainer } from "../../components/floating";
import { Icon } from "../../components/Icon";
import { useReducedMotion } from "../../components/motionHooks";
import { cx } from "../../lib/cx";
import { CachedImage } from "../images/CachedImage";
import { highlightParts } from "./highlight";
import type { Combobox } from "./useCombobox";

/**
 * The suggestion panel of a search field: the one floating list under the title-bar search and the
 * Torrents title field. The caller owns the text and what the rows mean; this draws groups of
 * rows, moves the highlight with the arrow keys, and speaks the combobox pattern to screen readers
 * (the field is the combobox, the panel its listbox, each row an option).
 */

/** One row. */
export interface SuggestItem {
  /** Unique among the rows of the panel. */
  id: string;
  /** The title; the typed words are marked in it. */
  label: string;
  poster?: PosterDto | null;
  /** A glyph in the poster's place (a search, a person). */
  icon?: LucideIcon;
  /** The muted line under the title: "Movie · 2024". */
  meta?: string;
  /** Shown at the row's end: a stage tag, "In your library". */
  badge?: ReactNode;
  /** A subtle remark after the title: "Similar". */
  note?: string;
  /** The title is only a near match for what was typed. */
  similar?: boolean;
}

export interface SuggestGroup {
  id: string;
  label: string;
  items: readonly SuggestItem[];
  /** A small button at the end of the heading (Clear). */
  action?: { label: string; onClick: () => void };
}

/** The title with the typed words in gold. */
function Marked({ text, term }: { text: string; term: string }) {
  return (
    <>
      {highlightParts(text, term).map((part, index) =>
        part.hit ? (
          <mark key={index} className="bg-transparent text-gold">
            {part.text}
          </mark>
        ) : (
          <span key={index}>{part.text}</span>
        ),
      )}
    </>
  );
}

function Thumb({ item }: { item: SuggestItem }) {
  if (item.icon !== undefined) {
    return (
      <span aria-hidden="true" className="grid h-9 w-6 shrink-0 place-items-center text-text-muted">
        <Icon icon={item.icon} />
      </span>
    );
  }
  return (
    <span
      aria-hidden="true"
      className="h-9 w-6 shrink-0 overflow-hidden rounded-(--r-4) bg-surface-active"
    >
      {item.poster !== undefined && item.poster !== null && (
        <CachedImage
          imageKey={item.poster.key}
          size="list"
          alt=""
          thumbhash={item.poster.thumbhash}
          ambient={item.poster.ambient}
          className="size-full"
        />
      )}
    </span>
  );
}

interface SuggestPanelProps<E extends HTMLElement> {
  combobox: Combobox<E>;
  /** The accessible name of the list. */
  label: string;
  groups: readonly SuggestGroup[];
  /** The text the rows are marked against. */
  term: string;
  /** A last row that goes to the full results ("See all 14 results"). */
  footer?: { id: string; label: string; icon?: LucideIcon };
  /** Shown in place of the rows when there are none ("No matches"). */
  empty?: ReactNode;
  /** A request is out: the rows shown are the last ones. */
  busy?: boolean;
  /** Minimum width of the panel. */
  minWidth?: number;
  className?: string;
}

/**
 * The panel under a field: the groups with their rows, then an optional last row. It fades in and
 * grows from the field in `--dur-pop` (at once with reduced motion), and keeps the last rows while
 * a new search is out, so it never blinks. Nothing is drawn when there is nothing to show.
 */
export function SuggestPanel<E extends HTMLElement>({
  combobox,
  label,
  groups,
  term,
  footer,
  empty,
  busy = false,
  minWidth = 320,
  className,
}: SuggestPanelProps<E>) {
  const container = useFloatingContainer();
  const reduced = useReducedMotion();
  const { open, anchor, attachPanel, activeId, setActive, select, optionId, listboxId } = combobox;
  const [box, setBox] = useState<{ left: number; top: number; width: number } | null>(null);
  const hasRows = groups.some((group) => group.items.length > 0) || footer !== undefined;
  const visible = open && (hasRows || empty !== undefined);

  // Hang the panel under the field, and keep it there when the window changes size.
  useLayoutEffect(() => {
    if (!visible) return;
    const place = () => {
      const rect = anchor?.getBoundingClientRect();
      if (rect === undefined) return;
      setBox({ left: rect.left, top: rect.bottom + 6, width: rect.width });
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [visible, anchor]);

  // The highlighted row stays in view while the arrow keys walk the list.
  useEffect(() => {
    if (activeId === null) return;
    document.getElementById(optionId(activeId))?.scrollIntoView?.({ block: "nearest" });
  }, [activeId, optionId]);

  const headingId = useId();
  if (!visible || box === null) return null;

  const row = (
    id: string,
    content: ReactNode,
    options: { className?: string; name: string },
  ): ReactNode => {
    const active = id === activeId;
    return (
      // The field keeps focus and points at the highlighted row (aria-activedescendant), so the
      // row is an option for pointers and screen readers, not a tab stop.
      // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/interactive-supports-focus
      <div
        key={id}
        id={optionId(id)}
        role="option"
        aria-label={options.name}
        aria-selected={active}
        data-active={active ? "" : undefined}
        onPointerMove={() => {
          if (!active) setActive(id);
        }}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => select(id)}
        className={cx(
          "flex min-h-10 cursor-pointer items-center gap-2.5 rounded-(--r-6) px-2.5 py-0.5",
          active ? "bg-surface-active" : "",
          options.className,
        )}
      >
        {content}
      </div>
    );
  };

  const body = (
    // The panel is a listbox of its field; keys go to the field, so it takes no focus itself.
    <div
      ref={attachPanel}
      id={listboxId}
      role="listbox"
      aria-label={label}
      aria-busy={busy}
      data-testid="suggest-panel"
      style={{
        position: "fixed",
        left: box.left,
        top: box.top,
        width: Math.max(box.width, minWidth),
        // Grows from the field's top edge; reduced motion shows it at once.
        ["--cv-origin" as string]: "50% 0",
        ...(reduced ? { animation: "none" } : {}),
      }}
      className={cx(
        floatingSurface,
        "max-h-[min(70vh,520px)] overflow-y-auto rounded-(--r-10) p-1 text-(length:--fs-13)",
        className,
      )}
    >
      {groups
        .filter((group) => group.items.length > 0)
        .map((group) => (
          <div key={group.id} role="group" aria-labelledby={`${headingId}-${group.id}`}>
            <div className="flex h-6 items-center px-2.5 pt-1">
              <span
                id={`${headingId}-${group.id}`}
                className="min-w-0 flex-1 truncate text-(length:--fs-11) font-(weight:--fw-600) tracking-wide text-text-muted uppercase"
              >
                {group.label}
              </span>
              {group.action !== undefined && (
                <button
                  type="button"
                  tabIndex={-1}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={group.action.onClick}
                  className="cv-hoverable cursor-pointer rounded-(--r-4) px-1.5 text-(length:--fs-12) text-text-muted hover:text-text"
                >
                  {group.action.label}
                </button>
              )}
            </div>
            {group.items.map((item) =>
              row(
                item.id,
                <>
                  <Thumb item={item} />
                  <span className="flex min-w-0 flex-1 flex-col">
                    <span className="truncate text-text">
                      <Marked text={item.label} term={term} />
                      {item.similar === true && item.note !== undefined && (
                        <span className="ml-2 text-(length:--fs-12) text-text-muted">
                          {item.note}
                        </span>
                      )}
                    </span>
                    {item.meta !== undefined && (
                      <span className="truncate text-(length:--fs-12) text-text-muted">
                        {item.meta}
                      </span>
                    )}
                  </span>
                  {item.badge}
                </>,
                {
                  name: [item.label, item.meta].filter(Boolean).join(", "),
                },
              ),
            )}
          </div>
        ))}
      {!hasRows && empty !== undefined && (
        <p role="status" className="m-0 px-2.5 py-3 text-text-muted">
          {empty}
        </p>
      )}
      {footer !== undefined &&
        row(
          footer.id,
          <>
            {footer.icon !== undefined && <Icon icon={footer.icon} />}
            <span className="truncate text-text-2">{footer.label}</span>
          </>,
          {
            name: footer.label,
            className:
              "sticky bottom-0 mt-1 min-h-9 rounded-t-none border-t border-solid border-line-section bg-surface-floating",
          },
        )}
    </div>
  );
  return createPortal(body, container ?? document.body);
}
