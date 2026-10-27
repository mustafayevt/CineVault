import { create } from "zustand";

/** One data source (a media folder) with its drive health. */
export interface SourceEntry {
  id: number;
  path: string;
  /** What the user sees for the folder: their label, which starts as the folder's last name. */
  label: string;
  /** False while the drive is disconnected; its items stay in the library. */
  online: boolean;
  count: number;
  /** Titles whose files are gone while the folder itself is reachable. */
  missing?: number;
}

/** Everything the sidebar shows. `null` counts are unknown and render nothing. */
export interface SidebarData {
  counts: { movies: number | null; tvShows: number | null; collections: number | null };
  /** The To do group. Both numbers come from the pipeline counts, like Home and Needs review. */
  todo: { needsReview: number | null; organize: number | null };
  sources: SourceEntry[];
}

export const EMPTY_SIDEBAR_DATA: SidebarData = {
  counts: { movies: null, tvShows: null, collections: null },
  todo: { needsReview: null, organize: null },
  sources: [],
};

interface SidebarDataState {
  data: SidebarData;
  /** Replaces the sidebar contents; fed by the library, view and source queries. */
  setData: (data: SidebarData) => void;
}

export const useSidebarDataStore = create<SidebarDataState>((set) => ({
  data: EMPTY_SIDEBAR_DATA,
  setData: (data) => set({ data }),
}));
