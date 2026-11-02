import { useQuery } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useCallback, useRef } from "react";

import { t } from "../../i18n/catalog";
import { runRescan, useScanUiStore } from "../scan/scanActions";
import { CollectionsView } from "./CollectionsView";
import { currentLibraryBackend } from "./libraryBackend";
import { LibraryToolbar } from "./LibraryToolbar";
import { COLUMN_LABEL, columnFits, SWITCHABLE_COLUMNS } from "./libraryColumns";
import { useLibraryStore } from "./libraryStore";
import type { LibraryType } from "./libraryStore";
import { LIBRARY_COUNTS_KEY } from "./useLibraryCounts";
import { TitlesView } from "./TitlesView";
import { usePaneFit } from "./usePaneFit";

const TYPES: readonly LibraryType[] = ["movies", "tv", "collections"];

/** The search of the address, as the router validated it. */
function searchOf(raw: unknown): { type: LibraryType | undefined; q: string } {
  const search = (typeof raw === "object" && raw !== null ? raw : {}) as Record<string, unknown>;
  const type = TYPES.find((entry) => entry === search.type);
  const q =
    typeof search.q === "string" ? search.q : typeof search.q === "number" ? String(search.q) : "";
  return { type, q };
}

/**
 * The Library (`/library`): the Movies | TV shows |
 * Collections switch, the toolbar, and under it the grid or the table with its filters, or the
 * collections. The type opens as the user left it, or as the address names it (`?type=`); the
 * title bar's search arrives as `?q=`.
 */
export function LibraryPage() {
  const navigate = useNavigate();
  const { type: routed, q } = searchOf(useSearch({ strict: false }));
  const remembered = useLibraryStore((store) => store.type);
  const type = routed ?? remembered;
  const store = useLibraryStore.getState();
  const state = useLibraryStore((all) => (type === "collections" ? all.movies : all[type]));
  const posterSize = useLibraryStore((all) => all.posterSize);
  const scanning = useScanUiStore((scan) => scan.starting);
  const sectionRef = useRef<HTMLElement>(null);
  const fit = usePaneFit(sectionRef);

  const counts = useQuery({
    queryKey: LIBRARY_COUNTS_KEY,
    queryFn: () => currentLibraryBackend().counts(),
    staleTime: 30_000,
  });

  const goTo = useCallback(
    (next: LibraryType) => {
      store.setType(next);
      void navigate({
        to: "/library",
        search: ((previous: Record<string, unknown>) => ({ ...previous, type: next })) as never,
      });
    },
    [navigate, store],
  );
  const clearText = useCallback(() => {
    void navigate({
      to: "/library",
      search: ((previous: Record<string, unknown>) => ({ ...previous, q: undefined })) as never,
    });
  }, [navigate]);

  const titles = type !== "collections";
  return (
    <section
      ref={sectionRef}
      aria-label={t("shell.page.library")}
      className="relative flex h-full min-h-0 flex-col"
    >
      <LibraryToolbar
        type={type}
        counts={{
          ...(counts.data === undefined
            ? {}
            : {
                movies: counts.data.movies,
                tv: counts.data.tv_shows,
                collections: counts.data.collections,
              }),
        }}
        onType={goTo}
        fit={fit}
        view={state.view}
        onView={(view) => titles && store.setView(type, view)}
        posterSize={posterSize}
        onPosterSize={store.setPosterSize}
        sort={state.sort}
        onSort={(sort) => titles && store.setSort(type, sort)}
        columns={SWITCHABLE_COLUMNS.filter((id) => columnFits(id, fit)).map((id) => ({
          id,
          label: t(COLUMN_LABEL[id]),
          shown: !state.hiddenColumns.includes(id),
        }))}
        onColumn={(id, shown) => titles && store.setColumnHidden(type, id, !shown)}
        scanning={scanning}
        onScan={() => void runRescan()}
      />
      {titles ? (
        <TitlesView key={type} type={type} text={q} onClearText={clearText} fit={fit} />
      ) : (
        <CollectionsView />
      )}
    </section>
  );
}
