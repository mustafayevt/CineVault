import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";

import type { DetailIdDto, LifecycleDto, PosterDto } from "../../bindings";
import { currentDetailBackend } from "../detail/detailBackend";
import { currentTvBackend } from "../tv/tvBackend";
import type { ScrapeKind } from "./scrapeBackend";

/**
 * What the scrape and artwork pages need to know about the title they work on: movie or show,
 * its name, the IDs it is matched by and where its files are. The address names only the item, so
 * the kind is found by asking for the movie first and the show when there is none.
 */
export interface TitleSubject {
  id: string;
  kind: ScrapeKind;
  title: string;
  year: number;
  ids: readonly DetailIdDto[];
  lifecycle: LifecycleDto | null;
  poster: PosterDto | null;
  backdrop: PosterDto | null;
  folder: string;
  runtimeMinutes: number;
  directors: readonly string[];
  plot: string;
  /** When the title was last written by a scrape or a rename is not known to the page. */
  availability: string;
  /** The main file of a movie, for the evidence line. */
  fileName: string;
  fileBytes: number | null;
  /** The length of the main file in seconds, when it has been read. */
  durationSeconds: number | null;
  /** How many NFO and artwork files the title has now. */
  metadataFiles: number;
}

const METADATA_KINDS: ReadonlySet<string> = new Set([
  "nfo",
  "poster",
  "fanart",
  "banner",
  "clearart",
  "clearlogo",
  "disc",
  "thumb",
  "keyart",
  "landscape",
]);

/** "Heat (1995)", or the title alone when the year is unknown. */
export function subjectName(subject: Pick<TitleSubject, "title" | "year">): string {
  return subject.year > 0 ? `${subject.title} (${subject.year})` : subject.title;
}

/** The provider IDs the title has now, as `tmdb:949`; none when it was never scraped. */
export function currentIdsOf(subject: Pick<TitleSubject, "ids" | "lifecycle">): string[] {
  // An ID read from a folder tag is not a match: the title has to be scraped.
  if (subject.lifecycle?.stage === "new") return [];
  return subject.ids.map((id) => `${id.provider}:${id.value}`);
}

async function loadMovie(queryClient: QueryClient, id: string): Promise<TitleSubject> {
  const movie = await queryClient.fetchQuery({
    queryKey: ["movie-detail", id],
    queryFn: () => currentDetailBackend().movie(id),
    staleTime: 30_000,
  });
  const video = movie.videos[0];
  return {
    id: movie.id,
    kind: "movie",
    title: movie.title,
    year: movie.year,
    ids: movie.ids,
    lifecycle: movie.lifecycle,
    poster: movie.poster,
    backdrop: movie.backdrop,
    folder: movie.folder,
    runtimeMinutes: movie.runtime_minutes,
    directors: movie.directors,
    plot: movie.plot,
    availability: movie.availability,
    fileName: video?.name ?? "",
    fileBytes: video?.size_bytes ?? null,
    durationSeconds: video !== undefined && video.duration_secs > 0 ? video.duration_secs : null,
    metadataFiles: movie.files.filter((file) => METADATA_KINDS.has(file.kind)).length,
  };
}

async function loadShow(queryClient: QueryClient, id: string): Promise<TitleSubject> {
  const show = await queryClient.fetchQuery({
    queryKey: ["tv-show", id],
    queryFn: () => currentTvBackend().show(id),
    staleTime: 30_000,
  });
  return {
    id: show.id,
    kind: "show",
    title: show.title,
    year: show.year,
    ids: show.ids,
    lifecycle: show.lifecycle,
    poster: show.poster,
    backdrop: show.backdrop,
    folder: show.folder,
    runtimeMinutes: show.runtime_minutes,
    directors: [],
    plot: show.plot,
    availability: show.availability,
    fileName: "",
    fileBytes: null,
    durationSeconds: null,
    metadataFiles: show.files.filter((file) => METADATA_KINDS.has(file.kind)).length,
  };
}

/** Reads a title by its library id: the movie, else the show. */
export async function loadTitleSubject(
  queryClient: QueryClient,
  id: string,
): Promise<TitleSubject> {
  try {
    return await loadMovie(queryClient, id);
  } catch (movieError) {
    try {
      return await loadShow(queryClient, id);
    } catch {
      throw movieError;
    }
  }
}

/** Query key of a title's subject. */
export function titleSubjectKey(id: string) {
  return ["title-subject", id] as const;
}

/** The title with this library id, as the pages that work on one title need it. */
export function useTitleSubject(id: string) {
  const queryClient = useQueryClient();
  return useQuery({
    queryKey: titleSubjectKey(id),
    queryFn: () => loadTitleSubject(queryClient, id),
    enabled: id !== "",
    staleTime: 0,
  });
}
