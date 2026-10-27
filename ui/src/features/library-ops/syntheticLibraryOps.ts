import type { MoveRowDto } from "../../bindings";
import type { LibrarySource } from "../library-sources/sourcesBackend";
import { LibraryOpsError } from "./libraryOpsBackend";
import type { LibraryOpsBackend } from "./libraryOpsBackend";

const GB = 1_000_000_000;

/**
 * The folders of the sample library, the ones of the sample drives. Movies is where
 * every sample title is now; Videos has no room and NAS Movies is offline.
 */
const SAMPLE_SOURCES: readonly LibrarySource[] = [
  { id: 1, kind: "movie", path: String.raw`D:\Movies`, label: "Movies", position: 0 },
  { id: 2, kind: "movie", path: String.raw`E:\Archive\Movies`, label: "Archive", position: 1 },
  { id: 3, kind: "tv", path: String.raw`E:\TV`, label: "TV", position: 0 },
  { id: 4, kind: "tv", path: String.raw`Z:\Shows`, label: "Shows", position: 1 },
  { id: 5, kind: "movie", path: String.raw`Z:\Movies`, label: "NAS Movies", position: 2 },
  { id: 6, kind: "movie", path: String.raw`C:\Users\Alex\Videos`, label: "Videos", position: 3 },
];

/** Free space the sample plan reports on a destination, in bytes. */
const SAMPLE_FREE: Readonly<Record<number, number | null>> = {
  2: 5.81e12,
  5: null,
  6: 41 * GB,
};

/** Every file and folder of the sample title, below its folder. */
const SAMPLE_ENTRIES: readonly { path: string; kind: string; size: number | null }[] = [
  { path: "Heat (1995) [2160p].mkv", kind: "file", size: 52 * GB },
  { path: "Heat (1995) [2160p].nfo", kind: "file", size: 14_000 },
  { path: "poster.jpg", kind: "file", size: 1_100_000 },
  { path: "fanart.jpg", kind: "file", size: 2_400_000 },
  { path: "Heat (1995) [2160p].eng.srt", kind: "file", size: 90_000 },
  { path: "Heat (1995) [2160p].tur.srt", kind: "file", size: 88_000 },
  { path: "Heat (1995)-trailer.mkv", kind: "file", size: 212_000_000 },
  { path: "extras", kind: "folder", size: null },
  { path: "extras/Making of.mkv", kind: "file", size: 9 * GB },
];

/** What the sample backend says about moving the sample title to a folder. */
function sampleRow(id: string, target: number): MoveRowDto {
  const source = SAMPLE_SOURCES.find((entry) => entry.id === target);
  if (source === undefined) throw new LibraryOpsError("unknown_source", "Unknown data source.");
  const bytes = SAMPLE_ENTRIES.reduce((sum, entry) => sum + (entry.size ?? 0), 0);
  const base: MoveRowDto = {
    id,
    kind: "movie",
    title: "Heat",
    year: 1995,
    source: "Movies",
    from: String.raw`C:\Users\Alex\Media\Heat (1995)`,
    to: `${source.path}\\Heat (1995)`,
    status: "ready",
    reason: "",
    files: SAMPLE_ENTRIES.filter((entry) => entry.kind === "file").length,
    bytes,
    across_volumes: !source.path.startsWith("C:"),
    needed_bytes: bytes + 2 * GB,
    free_bytes: [0, 0, 1800 * GB, 40 * GB, 900 * GB][target] ?? null,
    conflicts: [],
  };
  if (source.kind !== "movie") return { ...base, status: "wrong_kind", across_volumes: false };
  if (source.id === 1) return { ...base, status: "same_source", across_volumes: false };
  if (source.id === 3) return { ...base, status: "insufficient_space" };
  return base;
}

/**
 * A plain browser has no library. The sample backend lists believable previews and accepts every
 * action without keeping it, so the pages can be built and looked at.
 */
export function createSyntheticLibraryOpsBackend(): LibraryOpsBackend {
  const rows = (ids: readonly string[]) =>
    ids.map((id, index) => ({
      id,
      kind: "movie",
      title: ["Heat", "Ronin", "Alien", "Metropolis"][index % 4] ?? "Heat",
      year: [1995, 1998, 1979, 1927][index % 4] ?? 1995,
      source: "D:\\Movies",
      from: `D:\\Movies\\Title ${id}`,
      to: `E:\\Archive\\Title ${id}`,
      status: "ready",
      reason: "",
      files: 4,
      bytes: 8_400_000_000,
      across_volumes: true,
      needed_bytes: 8_500_000_000,
      free_bytes: null,
      conflicts: [],
    }));
  return {
    available: false,
    listSources: () => Promise.resolve(SAMPLE_SOURCES.map((source) => ({ ...source }))),
    previewTitle: (id, target) => {
      try {
        return Promise.resolve({
          row: sampleRow(id, target),
          entries: SAMPLE_ENTRIES.map((entry) => ({ ...entry, link_target: "" })),
        });
      } catch (error) {
        return Promise.reject(error as Error);
      }
    },
    controlMove: () => Promise.resolve(),
    planMove: (ids, target) => {
      const destination = SAMPLE_SOURCES.find((source) => source.id === target);
      const same = target === 1;
      const planned = rows(ids).map((row) => ({
        ...row,
        source: "Movies",
        to: `${destination?.path ?? ""}\\Title ${row.id}`,
        status: same ? "same_source" : "ready",
        bytes: 66.23 * GB,
        needed_bytes: same ? 0 : 66.23 * GB,
        across_volumes: !same,
        free_bytes: SAMPLE_FREE[target] ?? null,
      }));
      return Promise.resolve({
        target_id: target,
        target_path: destination?.path ?? "",
        rows: planned,
        ready: same ? 0 : ids.length,
      });
    },
    applyMove: (ids, _target, onProgress) => {
      ids.forEach((id, index) =>
        onProgress({
          done: index + 1,
          total: ids.length,
          title: `Title ${id}`,
          bytes_done: 0,
          bytes_total: 0,
          current_file: "",
          phase: "finishing",
          bytes_per_second: 0,
          seconds_left: null,
        }),
      );
      return Promise.resolve({
        batch_id: "sample",
        moved: ids.length,
        items: ids.map((id) => ({
          id,
          title: `Title ${id}`,
          result: "moved",
          reason: "",
          notes: [],
        })),
      });
    },
    undoBatch: (batchId) =>
      Promise.resolve({ batch_id: batchId, restored: 1, not_restorable: [], failed: [] }),
    planRemoval: (ids) =>
      Promise.resolve({
        rows: ids.map((id, index) => ({
          id,
          kind: "movie",
          title: ["Heat", "Ronin", "Alien"][index % 3] ?? "Heat",
          year: 1995,
          folder: `D:\\Movies\\Title ${id}`,
          folder_use: "own",
          files: 4,
          bytes: 8_400_000_000,
          children: 0,
          on_disk: true,
        })),
        skipped: [],
      }),
    removeFromLibrary: (ids, ignoreFolders) =>
      Promise.resolve({
        removed: ids.map((id) => ({
          id,
          title: `Title ${id}`,
          ignore: ignoreFolders ? "written" : "not_requested",
          ignore_reason: "",
        })),
        problems: [],
      }),
    deleteWithFiles: (ids) =>
      Promise.resolve({
        deleted: ids.map((id) => ({
          id,
          title: `Title ${id}`,
          method: "system_trash",
          paths: 1,
          notes: [],
        })),
        problems: [],
        batch_id: null,
      }),
  };
}
