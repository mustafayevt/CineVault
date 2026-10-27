import type { QueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";

import { t } from "../../i18n/catalog";
import { undoHistoryEntry } from "../history/historyUndo";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { currentReviewBackend } from "./reviewBackend";
import { currentScrapeBackend } from "./scrapeBackend";
import type { ScrapeKind, ScrapeRequest, ScrapeStreamHit } from "./scrapeBackend";
import { hitName, otherLibraryLink } from "./scrapeModel";
import { patchMovieDetail } from "./scrapeOptimistic";
import { titleSubjectKey } from "./titleSubject";

/** What a scrape of one title needs to know about it. */
export interface ScrapeSubjectRef {
  itemId: string;
  kind: ScrapeKind;
  title: string;
  year: number | null;
  /** The review row of a title already searched; accepting resolves it. */
  reviewId: number | null;
}

/** What a finished scrape leaves for its Undo. */
export interface ScrapeDone {
  /** Puts the title back as it was; `undefined` when nothing can be undone. */
  undo: (() => Promise<void>) | undefined;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The query keys that show a scraped title; they are read again once a scrape is done. */
function refreshScraped(queryClient: QueryClient, itemId: string): Promise<unknown> {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: ["library"] }),
    queryClient.invalidateQueries({ queryKey: ["movie-detail", itemId] }),
    queryClient.invalidateQueries({ queryKey: ["movie-nfo", itemId] }),
    queryClient.invalidateQueries({ queryKey: ["tv-show"] }),
    queryClient.invalidateQueries({ queryKey: ["review"] }),
    queryClient.invalidateQueries({ queryKey: ["lifecycle"] }),
    queryClient.invalidateQueries({ queryKey: titleSubjectKey(itemId) }),
  ]);
}

/**
 * Scrapes one title with the chosen candidate: a title waiting in the review queue is resolved
 * with it, any other is matched and its details, NFO and artwork written. Resolves to the way
 * back (the Undo of the toast). A title that already belongs to another library item is scraped
 * as a second version of it.
 */
async function scrapeWith(
  subject: ScrapeSubjectRef,
  hit: ScrapeStreamHit,
  language: string,
): Promise<ScrapeDone> {
  if (subject.reviewId !== null) {
    const reviewId = subject.reviewId;
    const review = currentReviewBackend();
    await review.accept(reviewId, {
      provider: hit.candidate.provider,
      providerId: hit.candidate.providerId,
    });
    return { undo: () => review.reopen([reviewId]) };
  }
  const backend = currentScrapeBackend();
  const selection = await backend.selection(subject.kind, "everything", []);
  const request: ScrapeRequest = {
    provider: hit.candidate.provider,
    providerId: hit.candidate.providerId,
    language,
    fields: selection.fields,
    doNotOverwrite: selection.doNotOverwrite,
    secondVersion: otherLibraryLink(hit, { itemId: subject.itemId }) !== null,
  };
  const applied = await backend.scrape(
    {
      itemId: subject.itemId,
      kind: subject.kind,
      title: subject.title,
      year: subject.year,
    },
    request,
  );
  // A change of match is a History entry, which Undo reads; a first scrape keeps its own copy.
  const entry = applied.historyEntry;
  if (entry !== null) {
    return {
      undo: async () => {
        await undoHistoryEntry(entry);
      },
    };
  }
  const token = applied.undo;
  return token === null ? { undo: undefined } : { undo: () => backend.undo(token) };
}

/**
 * Scrapes a title and reports it: the page shows the new match at once, a toast confirms with
 * Undo, and a refusal puts the old values back and says why. Resolves to whether it worked.
 */
export async function scrapeAndReport(
  queryClient: QueryClient,
  subject: ScrapeSubjectRef,
  hit: ScrapeStreamHit,
  language: string,
  onFailed?: () => void,
): Promise<boolean> {
  const name = hitName(hit.candidate);
  const rollback = patchMovieDetail(queryClient, subject.itemId, hit);
  try {
    const done = await scrapeWith(subject, hit, language);
    const undo = done.undo;
    showToast({
      glyph: RefreshCw,
      message: t("scrapePage.toast.scraped", { title: name }),
      ...(undo === undefined
        ? {}
        : {
            undo: async () => {
              await undo();
              await refreshScraped(queryClient, subject.itemId);
            },
          }),
    });
    await refreshScraped(queryClient, subject.itemId);
    return true;
  } catch (error) {
    rollback();
    notify({
      severity: "error",
      title: t("scrapePage.toast.couldNot", { title: name }),
      detail: messageOf(error),
    });
    onFailed?.();
    return false;
  }
}

/**
 * Scrapes every title with a confident top candidate in one backend call (spec `title-matching`
 * "Scrape page actions"). The backend decides which titles are confident, from the candidates it
 * found; the rest stay as they were. Resolves to the ids that worked and the ids that did not
 * (skipped and failed). One toast with Undo covers the whole run.
 */
export async function acceptConfident(
  queryClient: QueryClient,
  itemIds: readonly string[],
): Promise<{ done: string[]; failed: string[] }> {
  const review = currentReviewBackend();
  const backend = currentScrapeBackend();
  let result;
  try {
    result = await backend.acceptConfident(itemIds);
  } catch (error) {
    notify({
      severity: "error",
      title: t("scrapePage.toast.couldNotMany", { count: itemIds.length }),
      detail: messageOf(error),
    });
    return { done: [], failed: [...itemIds] };
  }
  const done = result.accepted.map((entry) => entry.itemId);
  const failed = [...result.skipped, ...result.failed];
  if (done.length > 0) {
    showToast({
      glyph: RefreshCw,
      message: t("scrapePage.toast.acceptedMany", { count: done.length }),
      undo: async () => {
        // One History entry covers the run; without one each title is put back by itself.
        if (result.historyEntry !== null) {
          await undoHistoryEntry(result.historyEntry);
          await Promise.all(done.map((id) => refreshScraped(queryClient, id)));
          return;
        }
        for (const entry of result.accepted) {
          if (entry.reviewId !== null) await review.reopen([entry.reviewId]);
          else if (entry.undo !== null) await backend.undo(entry.undo);
        }
        await Promise.all(done.map((id) => refreshScraped(queryClient, id)));
      },
    });
  }
  if (result.failed.length > 0) {
    notify({
      severity: "error",
      title: t("scrapePage.toast.couldNotMany", { count: result.failed.length }),
    });
  }
  await Promise.all(done.map((id) => refreshScraped(queryClient, id)));
  return { done, failed };
}
