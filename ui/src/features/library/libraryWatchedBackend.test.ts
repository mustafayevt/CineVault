import { beforeEach, describe, expect, it, vi } from "vitest";

const setWatched = vi.fn();
const toggleWatched = vi.fn();
const restoreWatched = vi.fn();

vi.mock("../../bindings", () => ({
  commands: {
    setWatched: (...args: unknown[]) => setWatched(...args),
    toggleWatched: (...args: unknown[]) => toggleWatched(...args),
    restoreWatched: (...args: unknown[]) => restoreWatched(...args),
  },
}));

const { LibraryRequestError, tauriLibraryBackend } = await import("./libraryBackend");

function report(previous: { id: string }[]) {
  return {
    covered: 5,
    changed: previous.length,
    previous,
    nfo_files_written: 0,
    nfo_problem: null,
  };
}

beforeEach(() => {
  setWatched.mockReset();
  toggleWatched.mockReset();
  restoreWatched.mockReset();
});

describe("the library's watched actions in the desktop app", () => {
  it("marks many titles and answers with their earlier states, for Undo", async () => {
    const before = [
      { id: "4", watched: true, play_count: 3, last_watched_ms: 5 },
      { id: "9", watched: false, play_count: 0, last_watched_ms: null },
    ];
    setWatched.mockResolvedValue({ status: "ok", data: report(before) });
    const previous = await tauriLibraryBackend.setWatched?.(["4", "5", "9"], true);
    expect(setWatched).toHaveBeenCalledWith(["4", "5", "9"], true);
    expect(previous).toEqual(before);
  });

  it("puts earlier states back through the restore command, play counts included", async () => {
    const before = [{ id: "4", watched: true, play_count: 3, last_watched_ms: 5 }];
    restoreWatched.mockResolvedValue({ status: "ok", data: report(before) });
    await tauriLibraryBackend.restoreWatched?.(before);
    expect(restoreWatched).toHaveBeenCalledWith(before);
  });

  it("turns a refused restore into an error", async () => {
    restoreWatched.mockResolvedValue({
      status: "error",
      error: { code: "storage", message: "The library is busy.", field: null },
    });
    await expect(
      tauriLibraryBackend.restoreWatched?.([
        { id: "4", watched: true, play_count: 3, last_watched_ms: null },
      ]),
    ).rejects.toThrow("The library is busy.");
  });

  it("toggles one title and answers with its new and earlier state", async () => {
    const before = { id: "4", watched: true, play_count: 2, last_watched_ms: null };
    toggleWatched.mockResolvedValue({
      status: "ok",
      data: { watched: false, report: report([before]) },
    });
    await expect(tauriLibraryBackend.toggleWatched?.("4")).resolves.toEqual({
      watched: false,
      previous: [before],
    });
    expect(toggleWatched).toHaveBeenCalledWith(["4"]);
  });

  it("turns a refusal into a request error", async () => {
    setWatched.mockResolvedValue({
      status: "error",
      error: { code: "storage", message: "The library is busy.", field: null },
    });
    await expect(tauriLibraryBackend.setWatched?.(["4"], true)).rejects.toBeInstanceOf(
      LibraryRequestError,
    );
  });
});
