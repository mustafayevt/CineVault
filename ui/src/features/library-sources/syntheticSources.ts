import { SourceRequestError } from "./sourcesBackend";
import type { LibrarySource, SourceKind, SourcesBackend } from "./sourcesBackend";

/**
 * An in-memory list of library folders, for developing the first-run setup in a plain browser
 * (`?first-run`). Nothing is stored beyond this tab, and nothing is scanned.
 */
export function createSyntheticSourcesBackend(seeded = false): SourcesBackend {
  // The folders of `SYNTHETIC_SOURCE_HEALTH` (scan/syntheticSources.ts), which say what they hold.
  let list: LibrarySource[] = seeded
    ? [
        {
          id: 1,
          kind: "movie",
          path: "C:\\Users\\alex\\Media\\Movies",
          label: "Movies",
          position: 0,
        },
        { id: 2, kind: "movie", path: "D:\\Archive", label: "Archive", position: 1 },
        { id: 3, kind: "tv", path: "Z:\\NAS\\TV Shows", label: "TV Shows", position: 0 },
      ]
    : [];
  let nextId = seeded ? 4 : 1;
  const unknown = () => Promise.reject(new SourceRequestError("unknown", "Unknown folder"));
  return {
    available: true,
    list: () => Promise.resolve([...list]),
    add: (kind: SourceKind, path: string) => {
      if (list.some((source) => source.kind === kind && source.path === path)) {
        return Promise.reject(new SourceRequestError("duplicate", `${path} is already added.`));
      }
      const created: LibrarySource = {
        id: nextId,
        kind,
        path,
        label: path.split(/[\\/]/).filter(Boolean).pop() ?? path,
        position: list.filter((source) => source.kind === kind).length,
      };
      nextId += 1;
      list = [...list, created];
      return Promise.resolve(created);
    },
    remove: (id) => {
      const found = list.find((source) => source.id === id);
      if (found === undefined) return unknown();
      list = list.filter((source) => source.id !== id);
      return Promise.resolve({ source: found, kept_items: 0 });
    },
    reorder: () => Promise.resolve([...list]),
    remap: () => unknown(),
    setKind: (id, kind) => {
      const found = list.find((source) => source.id === id);
      if (found === undefined) return unknown();
      const changed = { ...found, kind };
      list = list.map((source) => (source.id === id ? changed : source));
      return Promise.resolve(changed);
    },
    rename: (id, label) => {
      const found = list.find((source) => source.id === id);
      if (found === undefined) return unknown();
      if (list.some((s) => s.kind === found.kind && s.id !== id && s.label === label)) {
        return Promise.reject(new SourceRequestError("duplicate", `${label} is already used.`));
      }
      const renamed = { ...found, label };
      list = list.map((source) => (source.id === id ? renamed : source));
      return Promise.resolve(renamed);
    },
  };
}
