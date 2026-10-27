import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type { CollectionPageDto, EditCommandError } from "../../bindings";
import { syntheticRow } from "./syntheticLibrary";

export type CollectionPage = CollectionPageDto;

/** Where one collection with its owned and missing films comes from. */
export interface CollectionBackend {
  /** One collection with its films in release order; owned ones have an id, missing ones do not. */
  get: (id: string) => Promise<CollectionPage>;
}

/** A refusal from the backend: `message` is ready to show. */
class CollectionError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "CollectionError";
    this.code = code;
  }
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: EditCommandError };

const tauriCollectionBackend: CollectionBackend = {
  get: async (id) => {
    const result = (await commands.getCollection(id)) as Result<CollectionPage>;
    if (result.status === "error")
      throw new CollectionError(result.error.code, result.error.message);
    return result.data;
  },
};

/** Sample collections for a plain browser: the film list follows the library's own counts. */
const syntheticCollectionBackend: CollectionBackend = {
  get: (id) => {
    const index = Math.max(0, (Number.parseInt(id, 10) || 1) - 1);
    const row = syntheticRow(index, "collections");
    const owned = row.movies_in_set;
    const missing = row.missing_movies;
    const entries = Array.from({ length: owned + missing }, (_, i) => {
      const isOwned = i < owned;
      return {
        id: isOwned ? `${id}-film-${i}` : null,
        title: `${row.card.title} ${i + 1}`,
        year: 1979 + i * 7,
        release_date: null,
        watched: false,
        poster: null,
        poster_url: null,
        tmdb_id: null,
      };
    });
    return Promise.resolve({
      id,
      title: row.card.title,
      sort_title: row.sort_title,
      plot: "Science-fiction horror series following encounters between humanity and the Xenomorph.",
      tmdb_id: null,
      poster: row.card.poster,
      entries,
      owned,
      total: owned + missing,
    });
  },
};

let override: CollectionBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setCollectionBackend(backend: CollectionBackend | null): void {
  override = backend;
}

export function currentCollectionBackend(): CollectionBackend {
  if (override !== null) return override;
  return isTauri() ? tauriCollectionBackend : syntheticCollectionBackend;
}
