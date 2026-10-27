import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useToastStore } from "../toasts/toastStore";
import { setHistoryBackend } from "./historyBackend";
import type { HistoryBackend } from "./historyBackend";
import { createSyntheticHistoryBackend } from "./syntheticHistory";
import { leftText, undoBatchInHistory, undoHistoryEntry } from "./historyUndo";

function backendWith(overrides: Partial<HistoryBackend>): HistoryBackend {
  return { ...createSyntheticHistoryBackend(), ...overrides };
}

const REPORT = {
  entry_id: "1",
  undone: ["10", "11"],
  left: [
    { id: "12", title: "Heat (1995)", reason: 'Undo "Renamed 12 titles" first.' },
    { id: "13", title: "Alien (1979)", reason: "Its folder changed since." },
    { id: "14", title: "Ronin (1998)", reason: "Its drive is offline." },
  ],
  undo_state: "partial",
  undone_at: null,
};

beforeEach(() => {
  useToastStore.setState({ toasts: [], history: [] });
});
afterEach(() => setHistoryBackend(null));

describe("undoing a History entry", () => {
  it("names the titles it left, with their reasons, when only some came back", async () => {
    setHistoryBackend(backendWith({ undo: vi.fn(() => Promise.resolve(REPORT)) }));
    await undoHistoryEntry("1");
    const message = useToastStore.getState().toasts.at(-1)?.message ?? "";
    expect(message).toContain("Undid 2 of 5.");
    expect(message).toContain('Heat (1995): Undo "Renamed 12 titles" first.');
    expect(message).toContain("Alien (1979): Its folder changed since.");
    expect(message).toContain("and 1 more");
  });

  it("says nothing of its own when every title came back", async () => {
    setHistoryBackend(
      backendWith({
        undo: vi.fn(() => Promise.resolve({ ...REPORT, left: [], undo_state: "undone" })),
      }),
    );
    await undoHistoryEntry("1");
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("undoes one title's part through the same entry", async () => {
    const undoTitle = vi.fn(() => Promise.resolve({ ...REPORT, left: [] }));
    setHistoryBackend(backendWith({ undoTitle }));
    await undoHistoryEntry("1", "10", { announce: false });
    expect(undoTitle).toHaveBeenCalledWith("1", "10");
  });

  it("finds the entry of a journal batch, or null so the caller undoes the journal", async () => {
    const undo = vi.fn(() => Promise.resolve({ ...REPORT, left: [] }));
    setHistoryBackend(
      backendWith({
        entryForBatch: (batch) => Promise.resolve(batch === "b1" ? "7" : null),
        undo,
      }),
    );
    expect(await undoBatchInHistory("other")).toBeNull();
    expect((await undoBatchInHistory("b1"))?.undone).toEqual(["10", "11"]);
    expect(undo).toHaveBeenCalledWith("7");
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("lists at most two reasons", () => {
    expect(leftText(REPORT).split(" and 1 more")).toHaveLength(2);
  });
});
