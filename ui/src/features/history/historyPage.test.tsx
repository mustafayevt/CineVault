import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { HistoryEntryDto, HistoryTitleDto } from "../../bindings";
import { queryClient } from "../../queryClient";
import { restoreLayout, stubLayout } from "../../test/layout";
import { restorePlatform, stubPlatform } from "../../test/platform";
import { useToastStore } from "../toasts/toastStore";
import { setHistoryBackend } from "./historyBackend";
import type { HistoryBackend } from "./historyBackend";
import { HistoryPage } from "./HistoryPage";
import { groupByDay } from "./historyModel";

const GB = 1_000_000_000;
const DAY = 86_400_000;

function noon(daysAgo: number): number {
  const day = new Date();
  day.setHours(12, 0, 0, 0);
  return day.getTime() - daysAgo * DAY;
}

function title(id: string, before: string, after: string): HistoryTitleDto {
  return { id, title: `Title ${id}`, before, after, state: "done" };
}

function entry(
  id: string,
  kind: string,
  at: number,
  rest: Partial<HistoryEntryDto>,
): HistoryEntryDto {
  return {
    id,
    kind,
    at,
    summary: `Entry ${id}`,
    detail: "",
    bytes: null,
    undo_note: null,
    batch_id: null,
    undone_at: null,
    undo_state: "available",
    titles_total: 1,
    titles_undone: 0,
    preview: [],
    ...rest,
  };
}

const TWELVE = Array.from({ length: 12 }, (_, n) =>
  title(String(n + 1), `D:\\Movies\\old.${n + 1}`, `D:\\Movies\\Film ${n + 1} (2024)`),
);

function entries(): HistoryEntryDto[] {
  return [
    entry("1", "rename", noon(0), {
      summary: "Renamed 12 titles",
      detail: "12 folders, 41 files",
      titles_total: 12,
      preview: TWELVE.slice(0, 5),
    }),
    entry("2", "artwork", noon(0) - 1000, { summary: "Changed poster · Dune: Part Two" }),
    entry("3", "delete", noon(0) - 2000, {
      summary: "Deleted Godzilla (1998) to the Recycle Bin",
      detail: "E:\\Archive\\Movies\\GDZ",
    }),
    entry("4", "rename", noon(0) - 3000, {
      summary: "Renamed Solaris",
      undo_state: "undone",
      undone_at: noon(0),
      titles_undone: 1,
    }),
    entry("5", "delete_permanent", noon(1), {
      summary: "Permanently deleted 2 titles",
      detail: "18.4 GB · cannot be undone",
      undo_state: "none",
      titles_total: 2,
      preview: [title("30", "E:\\Cats (2019)", ""), title("31", "E:\\The Room (2003)", "")],
    }),
    entry("6", "match", noon(3), { summary: "Changed match · Heat (1986)" }),
  ];
}

interface Fake extends HistoryBackend {
  rows: HistoryEntryDto[];
  fire: () => void;
}

const KINDS: Record<string, string[]> = {
  all: [],
  renames: ["rename"],
  moves: ["move"],
  deletes: ["delete", "delete_permanent"],
  artwork_and_matches: ["artwork", "match"],
};

function fake(): Fake {
  const handlers = new Set<() => void>();
  const backend: Fake = {
    rows: entries(),
    list: vi.fn((filter, offset, limit) => {
      const kinds = KINDS[filter] ?? [];
      const all = backend.rows.filter((row) => kinds.length === 0 || kinds.includes(row.kind));
      return Promise.resolve({ entries: all.slice(offset, offset + limit), total: all.length });
    }),
    titles: vi.fn(() => Promise.resolve(TWELVE)),
    forTitle: vi.fn(() => Promise.resolve([])),
    entryForBatch: vi.fn(() => Promise.resolve(null)),
    binTotal: vi.fn(() => Promise.resolve(21.46 * GB)),
    undo: vi.fn((entryId: string) => {
      backend.rows = backend.rows.map((row) =>
        row.id === entryId ? { ...row, undo_state: "undone", undone_at: Date.now() } : row,
      );
      return Promise.resolve({
        entry_id: entryId,
        undone: ["1"],
        left: [],
        undo_state: "undone",
        undone_at: Date.now(),
      });
    }),
    undoTitle: vi.fn(),
    deleteTitles: vi.fn(),
    eraseTitles: vi.fn(),
    onChanged: (handler) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    fire: () => handlers.forEach((handler) => handler()),
  };
  return backend;
}

function renderPage() {
  // The app's own client: an Undo anywhere refreshes the page through it.
  render(
    <QueryClientProvider client={queryClient}>
      <HistoryPage />
    </QueryClientProvider>,
  );
}

let backend: Fake;
beforeEach(() => {
  stubLayout();
  stubPlatform("windows");
  queryClient.clear();
  backend = fake();
  setHistoryBackend(backend);
  useToastStore.setState({ toasts: [], history: [] });
});
afterEach(() => {
  restoreLayout();
  restorePlatform();
  setHistoryBackend(null);
});

const row = (summary: string) => {
  const found = screen.getByText(summary).closest<HTMLElement>("[data-kind]");
  if (found === null) throw new Error(`no row for ${summary}`);
  return found;
};

describe("the History page", () => {
  it("lists the entries newest first in day groups with the bin total in the header", async () => {
    renderPage();
    expect(await screen.findByText("Renamed 12 titles")).toBeInTheDocument();
    const today = screen.getByRole("region", { name: "Today" });
    expect(within(today).getAllByText(/^(Renamed|Changed|Deleted)/)).toHaveLength(4);
    expect(screen.getByRole("region", { name: "Yesterday" })).toBeInTheDocument();
    expect(await screen.findByTestId("bin-total")).toHaveTextContent("21.5 GB in Recycle Bin");
    expect(screen.getByRole("heading", { name: "History", level: 1 })).toBeInTheDocument();
  });

  it("names the bin the Trash on Linux, in the header total and its tip", async () => {
    stubPlatform("linux");
    renderPage();
    const total = await screen.findByTestId("bin-total");
    expect(total).toHaveTextContent("21.5 GB in Trash");
    expect(total).toHaveAttribute("title", "What CineVault sent to the Trash that is still there");
  });

  it("offers one action per kind: Undo all, Undo, Restore, Undone and No undo", async () => {
    renderPage();
    await screen.findByText("Renamed 12 titles");
    expect(
      within(row("Renamed 12 titles")).getByRole("button", { name: /^Undo all/ }),
    ).toBeVisible();
    expect(
      within(row("Changed poster · Dune: Part Two")).getByRole("button", { name: /^Undo:/ }),
    ).toBeVisible();
    expect(
      within(row("Deleted Godzilla (1998) to the Recycle Bin")).getByRole("button", {
        name: /^Restore/,
      }),
    ).toBeVisible();
    const undone = row("Renamed Solaris");
    expect(within(undone).getByText("Undone")).toBeInTheDocument();
    expect(within(undone).queryByRole("button")).toBeNull();
    expect(within(row("Permanently deleted 2 titles")).getByText("No undo")).toBeInTheDocument();
    // Only a permanent delete is drawn in the danger colour.
    expect(row("Permanently deleted 2 titles").innerHTML).toContain("text-danger");
    expect(row("Renamed 12 titles").innerHTML).not.toContain("text-danger");
  });

  it("filters by the tabs", async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText("Renamed 12 titles");
    const tabs = screen.getByRole("group", { name: "Show" });
    await user.click(within(tabs).getByRole("button", { name: "Deletes" }));
    await waitFor(() => expect(screen.queryByText("Renamed 12 titles")).toBeNull());
    expect(screen.getByText("Deleted Godzilla (1998) to the Recycle Bin")).toBeInTheDocument();
    expect(screen.getByText("Permanently deleted 2 titles")).toBeInTheDocument();
    expect(backend.list).toHaveBeenLastCalledWith("deletes", 0, expect.any(Number));
    await user.click(within(tabs).getByRole("button", { name: "Artwork and matches" }));
    expect(await screen.findByText("Changed match · Heat (1986)")).toBeInTheDocument();
  });

  it("undoes an entry from its button and shows it as Undone", async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText("Renamed 12 titles");
    await user.click(
      within(row("Changed poster · Dune: Part Two")).getByRole("button", { name: /^Undo:/ }),
    );
    await waitFor(() => expect(backend.undo).toHaveBeenCalledWith("2"));
    await waitFor(() =>
      expect(within(row("Changed poster · Dune: Part Two")).getByText("Undone")).toBeVisible(),
    );
    expect(useToastStore.getState().toasts.at(-1)?.message).toBe(
      "Undid Changed poster · Dune: Part Two",
    );
  });

  it("says Restored for a delete and names a title that was left, as a toast", async () => {
    const user = userEvent.setup();
    backend.undo = vi.fn(() =>
      Promise.resolve({
        entry_id: "3",
        undone: [],
        left: [
          {
            id: "9",
            title: "Godzilla (1998)",
            reason: "The files are no longer in the Recycle Bin; nothing was restored.",
          },
        ],
        undo_state: "available",
        undone_at: null,
      }),
    );
    setHistoryBackend(backend);
    renderPage();
    await screen.findByText("Renamed 12 titles");
    await user.click(
      within(row("Deleted Godzilla (1998) to the Recycle Bin")).getByRole("button", {
        name: /^Restore/,
      }),
    );
    await waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    expect(useToastStore.getState().toasts[0]?.message).toMatch(
      /Nothing was undone\. Godzilla \(1998\): The files are no longer in the Recycle Bin/,
    );
  });

  it("expands a batch to its first titles and lists the rest in place", async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText("Renamed 12 titles");
    expect(screen.queryByText("D:\\Movies\\old.1")).toBeNull();
    await user.click(screen.getByRole("button", { name: /Show the titles of Renamed 12 titles/ }));
    expect(screen.getByText("D:\\Movies\\old.1")).toBeInTheDocument();
    expect(screen.getByText("D:\\Movies\\Film 5 (2024)")).toBeInTheDocument();
    expect(screen.queryByText("D:\\Movies\\old.6")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Show 7 more" }));
    expect(await screen.findByText("D:\\Movies\\old.12")).toBeInTheDocument();
    expect(backend.titles).toHaveBeenCalledWith("1");
  });

  it("follows history-changed", async () => {
    renderPage();
    await screen.findByText("Renamed 12 titles");
    backend.rows = [
      entry("7", "move", noon(0) + 1000, {
        summary: "Moved 3 titles from D: to E:",
        titles_total: 3,
      }),
      ...backend.rows,
    ];
    act(() => backend.fire());
    expect(await screen.findByText("Moved 3 titles from D: to E:")).toBeInTheDocument();
  });

  it("says what it is for when nothing was recorded yet", async () => {
    backend.rows = [];
    renderPage();
    expect(await screen.findByText("Nothing here yet")).toBeInTheDocument();
  });
});

describe("grouping by day", () => {
  it("names today and yesterday and dates the rest", () => {
    const now = new Date(noon(0));
    const days = groupByDay(entries(), now, () => "20 Oct 2026");
    expect(days.map((day) => day.label)).toEqual(["Today", "Yesterday", "20 Oct 2026"]);
    expect(days[0]?.entries).toHaveLength(4);
  });
});
