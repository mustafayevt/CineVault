import { create } from "zustand";

/** What the Library's Reload media info button would act on, published by the title list. */
interface ReloadScope {
  /** The library type of the list the button sits over, or null when it shows no titles. */
  type: "movies" | "tv" | null;
  /** The selected titles; empty when nothing is selected. */
  selected: readonly string[];
  /** Every title of the current list (filters and search applied). */
  all: readonly string[];
}

export const useReloadScope = create<ReloadScope>(() => ({ type: null, selected: [], all: [] }));

export function publishReloadScope(scope: ReloadScope): void {
  useReloadScope.setState(scope);
}

export function clearReloadScope(): void {
  useReloadScope.setState({ type: null, selected: [], all: [] });
}
