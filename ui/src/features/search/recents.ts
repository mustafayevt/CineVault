import { useEffect } from "react";

import type { PosterDto } from "../../bindings";

/**
 * What the title-bar search remembers: the last searches and the last titles opened from it, six
 * of each, newest first, kept in the webview's storage. Damaged or missing storage reads as empty.
 */

export const RECENT_LIMIT = 6;
const SEARCHES = "cinevault.search.recent";
const OPENED = "cinevault.search.opened";

/** A title the person opened from the search. */
export interface OpenedTitle {
  kind: "movie" | "show";
  id: string;
  title: string;
  year: number;
  poster: PosterDto | null;
}

function read(key: string): unknown[] {
  try {
    const raw = window.localStorage.getItem(key);
    const value: unknown = raw === null ? [] : JSON.parse(raw);
    return Array.isArray(value) ? value : [];
  } catch {
    return [];
  }
}

function write(key: string, value: unknown[]): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // The list is a convenience; without storage it is simply not kept.
  }
}

export function readRecentSearches(): string[] {
  return read(SEARCHES)
    .filter((entry): entry is string => typeof entry === "string" && entry.trim() !== "")
    .slice(0, RECENT_LIMIT);
}

/** Puts a search at the front, once, without the older copy. */
export function pushRecentSearch(text: string): void {
  const clean = text.trim();
  if (clean === "") return;
  const lower = clean.toLowerCase();
  const rest = readRecentSearches().filter((entry) => entry.toLowerCase() !== lower);
  write(SEARCHES, [clean, ...rest].slice(0, RECENT_LIMIT));
}

export function clearRecentSearches(): void {
  write(SEARCHES, []);
}

function isOpened(entry: unknown): entry is OpenedTitle {
  if (typeof entry !== "object" || entry === null) return false;
  const value = entry as Record<string, unknown>;
  return (
    (value.kind === "movie" || value.kind === "show") &&
    typeof value.id === "string" &&
    typeof value.title === "string" &&
    typeof value.year === "number"
  );
}

export function readOpenedTitles(): OpenedTitle[] {
  return read(OPENED).filter(isOpened).slice(0, RECENT_LIMIT);
}

/** Puts a title at the front, once. */
export function pushOpenedTitle(title: OpenedTitle): void {
  const rest = readOpenedTitles().filter(
    (entry) => !(entry.kind === title.kind && entry.id === title.id),
  );
  write(OPENED, [title, ...rest].slice(0, RECENT_LIMIT));
}

export function clearOpenedTitles(): void {
  write(OPENED, []);
}

/** Remembers the title of an open page, so the search can offer it again under "Recently opened". */
export function useRememberOpened(title: OpenedTitle): void {
  const { kind, id, title: name, year, poster } = title;
  useEffect(() => {
    if (id !== "") pushOpenedTitle({ kind, id, title: name, year, poster });
  }, [kind, id, name, year, poster]);
}
