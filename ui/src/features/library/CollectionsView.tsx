import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent } from "react";

import { PlaceholderPoster } from "../../components/PlaceholderPoster";
import { Skeleton } from "../../components/Skeleton";
import { useListSelection } from "../../components/useListSelection";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { CachedImage } from "../images/CachedImage";
import { CollectionPane } from "./CollectionPane";
import { useLibraryListing } from "./useLibraryListing";
import type { LibraryQuery, LibraryRow } from "./libraryBackend";
import "./library.css";

/** The collections, by name. */
const QUERY: LibraryQuery = {
  kind: "collections",
  sort: [{ field: "title", descending: false }],
  search: "",
  filters: [],
};

const MIN_TILE = 130;
const GAP = 18;

/** One short bar per film of the set, the owned ones lit. */
function Dots({ owned, total }: { owned: number; total: number }) {
  return (
    <span aria-hidden="true" className="flex min-w-0 gap-[3px] overflow-hidden">
      {Array.from({ length: Math.min(total, 12) }, (_, i) => (
        <span
          key={i}
          className={cx(
            "h-[3px] w-3.5 shrink-0 rounded-[1.5px]",
            i < owned ? "bg-text-2" : "bg-line-edge",
          )}
        />
      ))}
    </span>
  );
}

function Tile({
  row,
  index,
  selected,
  current,
  onSelect,
}: {
  row: LibraryRow;
  index: number;
  selected: boolean;
  current: boolean;
  onSelect: () => void;
}) {
  const card = row.card;
  const owned = row.movies_in_set;
  const total = row.movies_in_set + row.missing_movies;
  return (
    <div
      id={`collection-${index}`}
      data-index={index}
      data-id={card.id}
      data-current={current ? "true" : undefined}
      className="cv-collection-tile min-w-0"
    >
      <button
        type="button"
        aria-pressed={selected}
        tabIndex={-1}
        onClick={onSelect}
        className="group flex w-full min-w-0 flex-col gap-2 border-0 bg-transparent p-0 text-left text-text"
      >
        <span
          className={cx(
            "cv-collection-stack relative mt-2 mr-2.5 block aspect-[2/3] rounded-(--r-10)",
            "outline-offset-4",
            selected ? "outline-2 outline-gold" : "outline-0",
          )}
        >
          {/* Two sheets behind the poster: a set, not a single film. */}
          <span className="absolute inset-0 translate-x-2.5 -translate-y-2 rounded-(--r-10) bg-surface-active opacity-45" />
          <span className="absolute inset-0 translate-x-1 -translate-y-1 rounded-(--r-10) bg-surface-active opacity-75" />
          <span className="cv-lift absolute inset-0 overflow-hidden rounded-(--r-10) shadow-floating">
            {card.poster === null ? (
              <PlaceholderPoster title={card.title} />
            ) : (
              <CachedImage
                imageKey={card.poster.key}
                size="grid"
                alt=""
                thumbhash={card.poster.thumbhash}
                ambient={card.poster.ambient}
                className="size-full"
              />
            )}
          </span>
        </span>
        <span className="truncate font-(weight:--fw-600)">{card.title}</span>
        <span className="-mt-1 flex items-center gap-2 text-(length:--fs-12) text-text-muted">
          <span className="shrink-0 whitespace-nowrap">
            {t("library.collections.owned", { owned, total })}
          </span>
          <Dots owned={owned} total={total} />
        </span>
      </button>
    </div>
  );
}

/**
 * The Collections view: a grid of collection tiles, "3 of 5 owned", by name,
 * and the docked pane of the one chosen. Collections form by themselves once two films of a TMDB
 * set are owned, so there is nothing to create or edit here; only the artwork can change.
 */
export function CollectionsView() {
  const listing = useLibraryListing(QUERY);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);

  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (element === null) return;
    const measure = () => setWidth(element.clientWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const columns = Math.max(1, Math.floor((Math.max(0, width - 48) + GAP) / (MIN_TILE + GAP)));

  // Collections are few: load every page the list has.
  const { setRange, total, keys } = listing;
  useEffect(() => {
    if (total > 0) setRange({ start: 0, end: total - 1 });
  }, [total, setRange]);

  const selection = useListSelection({ keys, columns });
  const { select, count } = selection;

  // Open on the first collection, so the pane is never an empty frame beside a full grid.
  const auto = useRef(false);
  const first = keys[0];
  useEffect(() => {
    if (!auto.current && count === 0 && first !== undefined && !first.startsWith("#")) {
      auto.current = true;
      select([first]);
    }
  }, [count, first, select]);

  const currentIndex = selection.currentIndex;
  const chosen = useMemo(
    () => (currentIndex >= 0 ? listing.rowAt(currentIndex) : undefined),
    // `rowAt` reads the pages whose arrival is `version`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [currentIndex, listing.version],
  );

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return;
    // Choosing is the one action: Enter, Space and the range keys have nothing to do here.
    if (event.key === "Enter" || event.key === " " || event.shiftKey || event.ctrlKey) return;
    if (selection.onKeyDown(event)) event.preventDefault();
  };

  /* eslint-disable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */
  return (
    <div className="flex min-h-0 flex-1">
      <div
        ref={scrollRef}
        role="group"
        aria-label={t("library.collections.grid")}
        aria-busy={listing.loading}
        aria-activedescendant={currentIndex >= 0 ? `collection-${currentIndex}` : undefined}
        tabIndex={0}
        onKeyDown={onKeyDown}
        className="cv-collection-grid min-h-0 min-w-0 flex-1 overflow-x-hidden overflow-y-auto px-6 pt-[18px] pb-6 outline-none select-none"
      >
        {total === 0 && !listing.loading ? (
          <div className="flex flex-col items-start gap-2 py-8">
            <p className="m-0 text-(length:--fs-14) font-(weight:--fw-600)">
              {t("library.collections.noneTitle")}
            </p>
            <p className="m-0 text-text-2">{t("library.collections.noneBody")}</p>
          </div>
        ) : (
          <div
            className="grid gap-x-[18px] gap-y-[22px]"
            style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}
          >
            {Array.from({ length: total }, (_, index) => {
              const row = listing.rowAt(index);
              const key = keys[index] ?? `#${index}`;
              if (row === undefined) {
                return (
                  <div key={key} className="pt-2 pr-2.5">
                    <Skeleton shape="poster" />
                  </div>
                );
              }
              return (
                <Tile
                  key={key}
                  row={row}
                  index={index}
                  selected={selection.isSelected(key)}
                  current={index === currentIndex}
                  onSelect={() => {
                    scrollRef.current?.focus();
                    selection.click(key);
                  }}
                />
              );
            })}
          </div>
        )}
      </div>
      {chosen !== undefined && <CollectionPane key={chosen.card.id} row={chosen} />}
    </div>
  );
}
/* eslint-enable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */
