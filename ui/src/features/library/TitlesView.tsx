import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";

import { Button } from "../../components/Button";
import { useListSelection } from "../../components/useListSelection";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { clearReloadScope, publishReloadScope } from "../mediainfo/reloadScope";
import { usePreferredSource } from "../ratings/preference";
import { parseQuery } from "../search/queryParser";
import { isTextField } from "../delete/deleteKeys";
import { useListingWithoutHidden } from "../delete/hiddenTitles";
import { LibraryFilterRow } from "./LibraryFilterRow";
import { LibrarySelectionBar } from "./LibrarySelectionBar";
import { LibraryTable } from "./LibraryTable";
import { currentLibraryBackend } from "./libraryBackend";
import { filterChips, problemChips, titleQuery, useLibraryStore } from "./libraryStore";
import type { TitleType } from "./libraryStore";
import { PosterGrid } from "./PosterGrid";
import { TitleContextMenu } from "./TitleContextMenu";
import { useLibraryActions } from "./useLibraryActions";
import type { TitleTarget } from "./useLibraryActions";
import { useLibraryListing } from "./useLibraryListing";

interface TitlesViewProps {
  type: TitleType;
  /** The typed search of the title bar (`?q=`). */
  text: string;
  onClearText: () => void;
}

/**
 * The movies or TV shows of the Library: the filter row, the grid or the table over one listing,
 * the floating selection bar and the right-click menu, all on one selection. Filters, sort and
 * selection are kept when the view switches (spec library-browsing "Grid and table views").
 */
export function TitlesView({ type, text, onClearText }: TitlesViewProps) {
  const navigate = useNavigate();
  const format = useFormat();
  const kind = type === "movies" ? "movies" : "shows";
  const state = useLibraryStore((store) => store[type]);
  const { setFilters, clearFilters, setSort, setColumnHidden } = useLibraryStore.getState();
  const ratingSource = usePreferredSource();

  const typed = useMemo(() => parseQuery(text), [text]);
  const baseChips = useMemo(
    () => [...filterChips(state.filters), ...typed.chips],
    [state.filters, typed.chips],
  );
  const query = useMemo(
    () => titleQuery(type, state.sort, baseChips, typed.text, ratingSource),
    [type, state.sort, baseChips, typed.text, ratingSource],
  );
  const union = useMemo(
    () =>
      state.filters.problems === "all"
        ? problemChips("all").map((chips) =>
            titleQuery(type, state.sort, [...baseChips, ...chips], typed.text, ratingSource),
          )
        : null,
    [state.filters.problems, type, state.sort, baseChips, typed.text, ratingSource],
  );
  // One problem filter is a chip of the one query; "All problems" is the union of the three.
  const single = useMemo(() => {
    if (state.filters.problems === "any" || state.filters.problems === "all") return query;
    const [chips] = problemChips(state.filters.problems);
    return { ...query, filters: [...query.filters, ...(chips ?? [])] };
  }, [query, state.filters.problems]);

  const listing = useListingWithoutHidden(useLibraryListing(single, union));
  const keys = listing.keys;

  const open = useCallback(
    (id: string) => {
      if (type === "movies") void navigate({ to: "/movies/$movieId", params: { movieId: id } });
      else void navigate({ to: "/tv/$showId", params: { showId: id } });
    },
    [type, navigate],
  );
  // The grid steps by rows, so the selection needs to know how many posters fit in one.
  const [gridColumns, setGridColumns] = useState(1);
  const selection = useListSelection({
    keys,
    onOpen: open,
    columns: state.view === "grid" ? gridColumns : 1,
  });

  const targets = useMemo<TitleTarget[]>(
    () =>
      [...selection.selected].map((id) => {
        const known = listing.seen.get(id);
        return {
          id,
          kind: type === "movies" ? "movie" : "show",
          title: known?.title ?? "",
          folder: known?.folder ?? "",
          filesMissing: known?.filesMissing ?? false,
        };
      }),
    [selection.selected, listing.seen, type],
  );
  const actions = useLibraryActions(targets);

  // The toolbar's Reload media info button acts on the selection, or on this whole list.
  const selectedIds = useMemo(() => [...selection.selected], [selection.selected]);
  useEffect(() => {
    publishReloadScope({ type, selected: selectedIds, all: keys });
  }, [type, selectedIds, keys]);
  useEffect(() => clearReloadScope, []);

  const filtered = baseChips.length > 0 || typed.text !== "" || state.filters.problems !== "any";
  const everything = useQuery({
    queryKey: ["library", "size-total", kind] as const,
    queryFn: () => currentLibraryBackend().sizeTotal?.(kind) ?? Promise.resolve(null),
    staleTime: 30_000,
    enabled: !filtered,
  });
  const totalBytes = filtered ? listing.totalBytes : (everything.data?.counted_bytes ?? null);
  const noun = t(type === "movies" ? "library.footer.movies" : "library.footer.shows", {
    count: listing.total,
  });
  const footer = listing.loading
    ? ""
    : totalBytes === null
      ? noun
      : `${noun} · ${format.size(totalBytes)}`;

  const selectedBytes = useMemo(() => {
    let sum = 0;
    for (const id of selection.selected) {
      const bytes = listing.bytesOf(id);
      if (bytes === undefined) return undefined;
      sum += bytes;
    }
    return format.size(sum);
  }, [selection.selected, listing, format]);

  /** F2, Delete and Shift+Delete on the list; Enter, arrows, Ctrl+A and Esc are the selection's. */
  const onKey = useCallback(
    (event: KeyboardEvent | React.KeyboardEvent): boolean => {
      if (selection.count === 0 || isTextField(event.target)) return false;
      const act = (id: "rename" | "delete" | "delete-permanently") => {
        const action = actions[id];
        if (action.disabledReason === undefined) action.onSelect();
        return true;
      };
      if (event.key === "F2") return act("rename");
      if (event.key === "Delete") return act(event.shiftKey ? "delete-permanently" : "delete");
      return false;
    },
    [selection.count, actions],
  );

  const empty: ReactNode = filtered ? (
    <div className="flex flex-col items-start gap-2 py-8">
      <p className="m-0 text-(length:--fs-14) font-(weight:--fw-600) text-text">
        {t("library.filtered.title")}
      </p>
      <p className="m-0 text-text-2">{t("library.filtered.body")}</p>
      <Button
        variant="default"
        onClick={() => {
          clearFilters(type);
          onClearText();
        }}
      >
        {t("library.filtered.reset")}
      </Button>
    </div>
  ) : (
    <div className="flex flex-col items-start gap-2 py-8">
      <p className="m-0 text-(length:--fs-14) font-(weight:--fw-600) text-text">
        {t("library.empty.title")}
      </p>
      <p className="m-0 text-text-2">{t("library.empty.body")}</p>
      <Button variant="default" onClick={() => void navigate({ to: "/settings" })}>
        {t("library.empty.action")}
      </Button>
    </div>
  );

  const menu = <TitleContextMenu actions={actions} />;
  // The bar covers the last row: leave its height (44 plus the 18px gap and 12px air) under it.
  const bottomRoom = selection.count > 0 ? 74 : 16;

  return (
    <>
      <LibraryFilterRow
        kind={kind}
        filters={state.filters}
        onChange={(change) => setFilters(type, change)}
        text={text}
        onClearText={onClearText}
        footer={footer}
      />
      {state.view === "grid" ? (
        <PosterGrid
          type={type}
          listing={listing}
          selection={selection}
          menu={menu}
          bottomRoom={bottomRoom}
          empty={empty}
          onKey={onKey}
          onColumns={setGridColumns}
        />
      ) : (
        <LibraryTable
          type={type}
          listing={listing}
          selection={selection}
          menu={menu}
          bottomRoom={bottomRoom}
          empty={empty}
          sort={state.sort}
          onSort={(sort) => setSort(type, sort)}
          hiddenColumns={state.hiddenColumns}
          onColumn={(column, shown) => setColumnHidden(type, column, !shown)}
          onKey={onKey}
        />
      )}
      <LibrarySelectionBar
        actions={actions}
        count={selection.count}
        size={selectedBytes}
        onClear={selection.clear}
      />
    </>
  );
}
