import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { queryClient } from "../../queryClient";
import { useMoveToStore } from "../move-to/moveToStore";
import { OPEN_RENAME_ACTION } from "../rename/runRename";

const navigate = vi.hoisted(() => vi.fn(() => Promise.resolve()));
vi.mock("../../router", () => ({ router: { navigate } }));
import { useNotificationStore } from "../notifications/notificationStore";
import type { AppNotification } from "../notifications/notificationStore";
import { useToastStore } from "../toasts/toastStore";
import { attentionEntries } from "./attentionModel";
import { NeedsAttention } from "./NeedsAttention";
import { setRecoveryBackend } from "./recoveryBackend";
import type { RecoveryBackend, ResumeOutcome, UnfinishedBatch } from "./recoveryBackend";
import {
  autoResumeUnfinished,
  batchLabel,
  batchProgress,
  resetAutoResume,
  useRecoveryStore,
} from "./resolveUnfinished";

const GB = 1_000_000_000;

function batch(id: string, rest: Partial<UnfinishedBatch> = {}): UnfinishedBatch {
  return {
    batch_id: id,
    kind: "move",
    created_ms: Date.now(),
    titles: ["Dune: Part Two"],
    items: 1,
    files_done: 8,
    files_total: 10,
    reason: "CineVault closed before the move finished.",
    item_ids: ["7"],
    destination: String.raw`F:\Movie`,
    bytes_done: 15 * GB,
    bytes_total: 86 * GB,
    auto_resume: false,
    ...rest,
  };
}

function notice(id: string, rest: Partial<AppNotification> = {}): AppNotification {
  return { id, severity: "warning", title: `Notice ${id}`, at: Date.now(), ...rest };
}

interface Fake extends RecoveryBackend {
  batches: UnfinishedBatch[];
}

function fake(batches: UnfinishedBatch[]): Fake {
  const backend: Fake = {
    batches,
    list: vi.fn(() => Promise.resolve(backend.batches)),
    resume: vi.fn((batchId: string, onProgress) => {
      onProgress({
        done: 0,
        total: 1,
        title: "Dune: Part Two",
        bytes_done: 43 * GB,
        bytes_total: 86 * GB,
        current_file: "Dune.mkv",
        phase: "copying",
        bytes_per_second: null,
        seconds_left: null,
      });
      backend.batches = backend.batches.filter((entry) => entry.batch_id !== batchId);
      return Promise.resolve({ batch_id: batchId, completed: 1, stopped: null, notes: [] });
    }),
    rollBack: vi.fn((batchId: string) => {
      backend.batches = backend.batches.filter((entry) => entry.batch_id !== batchId);
      return Promise.resolve({ batch_id: batchId, restored: 1, removed: 9, problems: [] });
    }),
  };
  return backend;
}

function renderSection(focusBatch: string | null = null) {
  render(
    <QueryClientProvider client={queryClient}>
      <NeedsAttention focusBatch={focusBatch} />
    </QueryClientProvider>,
  );
}

const toasts = () => useToastStore.getState().toasts.map((toast) => toast.message);

let backend: Fake;
beforeEach(() => {
  queryClient.clear();
  backend = fake([]);
  setRecoveryBackend(backend);
  useNotificationStore.getState().clear();
  useMoveToStore.setState({ running: null, request: null });
  useRecoveryStore.setState({ active: null, working: {} });
  resetAutoResume();
});
afterEach(() => {
  setRecoveryBackend(null);
});

describe("what needs attention", () => {
  it("lists unfinished batches first, then warnings and errors, and leaves information out", () => {
    const entries = attentionEntries([batch("move-1")], null, [
      notice("n1", { severity: "error" }),
      notice("n2", { severity: "info" }),
      notice("n3"),
    ]);
    expect(entries.map((entry) => entry.key)).toEqual(["batch:move-1", "notice:n1", "notice:n3"]);
  });

  it("drops the notices about unfinished batches once the list is read, and keeps them until then", () => {
    const about = notice("n1", { action: "resolve_unfinished", batchId: "move-1" });
    expect(attentionEntries([batch("move-1")], null, [about]).map((e) => e.key)).toEqual([
      "batch:move-1",
    ]);
    expect(attentionEntries([], null, [about])).toEqual([]);
    expect(attentionEntries(undefined, null, [about]).map((e) => e.key)).toEqual(["notice:n1"]);
  });

  it("keeps the batch being resumed, which the backend stops listing while it runs", () => {
    const running = batch("move-1");
    expect(attentionEntries([], running, []).map((e) => e.key)).toEqual(["batch:move-1"]);
    expect(attentionEntries([running], running, [])).toHaveLength(1);
  });

  it("names a batch by its titles and says how far it got", () => {
    const size = (bytes: number) => `${bytes / GB} GB`;
    expect(batchLabel(batch("a"))).toBe("Dune: Part Two");
    expect(batchLabel(batch("a", { titles: ["Heat", "Alien"], items: 3 }))).toBe("Heat and 2 more");
    expect(batchLabel(batch("a", { titles: [], items: 2 }))).toBe("2 titles");
    expect(batchProgress(batch("a"), size)).toBe("8 of 10 files copied, 15 GB of 86 GB");
    expect(batchProgress(batch("a", { bytes_total: null }), size)).toBe("8 of 10 files copied");
    expect(batchProgress(batch("a", { kind: "rename", files_done: 2, files_total: 5 }), size)).toBe(
      "2 of 5 titles renamed",
    );
  });
});

describe("Needs attention", () => {
  it("is not shown while nothing needs the user", async () => {
    renderSection();
    await waitFor(() => expect(backend.list).toHaveBeenCalled());
    expect(screen.queryByTestId("needs-attention")).toBeNull();
  });

  it("shows an unfinished move with where it was going, how far it got and why it stopped", async () => {
    backend.batches = [batch("move-1")];
    renderSection();
    const section = await screen.findByRole("region", { name: "Needs attention" });
    expect(within(section).getByText(String.raw`Move to F:\Movie did not finish`)).toBeVisible();
    expect(within(section).getByText(/8 of 10 files copied, 15 GB of 86 GB/)).toBeVisible();
    expect(within(section).getByText("CineVault closed before the move finished.")).toBeVisible();
    expect(within(section).getByText("1 item")).toBeVisible();
  });

  it("resumes the move, shows its progress while it runs and offers Undo when it finishes", async () => {
    backend.batches = [batch("move-1")];
    let finish: (() => void) | undefined;
    backend.resume = vi.fn(
      (batchId: string, onProgress: Parameters<RecoveryBackend["resume"]>[1]) =>
        new Promise<ResumeOutcome>((resolve) => {
          onProgress({
            done: 0,
            total: 1,
            title: "Dune: Part Two",
            bytes_done: 43 * GB,
            bytes_total: 86 * GB,
            current_file: "Dune.mkv",
            phase: "copying",
            bytes_per_second: null,
            seconds_left: null,
          });
          finish = () => {
            backend.batches = [];
            resolve({ batch_id: batchId, completed: 1, stopped: null, notes: [] });
          };
        }),
    );
    renderSection();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Resume" }));

    expect(backend.resume).toHaveBeenCalledWith("move-1", expect.any(Function));
    expect(await screen.findByText(String.raw`Resuming the move to F:\Movie`)).toBeVisible();
    expect(screen.getByRole("progressbar")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Pause" })).toBeVisible();
    expect(useMoveToStore.getState().running).toMatchObject({
      batchId: "move-1",
      percent: 50,
    });

    await act(async () => finish?.());
    await waitFor(() => expect(screen.queryByTestId("needs-attention")).toBeNull());
    expect(useMoveToStore.getState().running).toBeNull();
    const done = useToastStore.getState().toasts.at(-1);
    expect(done?.message).toBe("Finished the move of Dune: Part Two");
    expect(done?.undo).toBeDefined();
  });

  it("rolls the move back and says so", async () => {
    backend.batches = [batch("move-1")];
    renderSection();
    await userEvent.setup().click(await screen.findByRole("button", { name: "Roll back" }));
    expect(backend.rollBack).toHaveBeenCalledWith("move-1");
    await waitFor(() => expect(screen.queryByTestId("needs-attention")).toBeNull());
    expect(toasts()).toContain("Rolled back the move of Dune: Part Two");
  });

  it("does not offer Resume while another move runs", async () => {
    backend.batches = [batch("move-1")];
    useMoveToStore.setState({
      running: { label: "Heat", target: "E: Archive", percent: 10, paused: false },
    });
    renderSection();
    expect(await screen.findByRole("button", { name: "Resume" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Roll back" })).toBeEnabled();
  });

  it("lists warnings and errors with Dismiss and Dismiss all, and their fix when they have one", async () => {
    const store = useNotificationStore.getState();
    store.add({ severity: "error", title: "TMDB rejected the API key", providerKey: "tmdb" });
    store.add({ severity: "warning", title: "Artwork could not be saved", detail: "Heat: denied" });
    store.add({ severity: "info", title: "Moved 3 titles" });
    renderSection();
    const section = await screen.findByRole("region", { name: "Needs attention" });
    expect(within(section).getByText("TMDB rejected the API key")).toBeVisible();
    expect(within(section).getByText("Heat: denied")).toBeVisible();
    expect(within(section).queryByText("Moved 3 titles")).toBeNull();
    expect(within(section).getByRole("button", { name: "Open the key" })).toBeVisible();

    const user = userEvent.setup();
    await user.click(
      within(section).getByRole("button", { name: "Dismiss “Artwork could not be saved”" }),
    );
    expect(within(section).queryByText("Artwork could not be saved")).toBeNull();
    await user.click(within(section).getByRole("button", { name: "Dismiss all" }));
    expect(screen.queryByTestId("needs-attention")).toBeNull();
  });

  it("opens the titles a rename could not rename in Rename, to try them again", async () => {
    useNotificationStore.getState().add({
      severity: "error",
      title: "2 titles could not be renamed",
      detail: "Heat: the file is in use by another program\nAlien: access denied",
      action: OPEN_RENAME_ACTION,
      itemIds: ["4", "9"],
    });
    renderSection();
    const section = await screen.findByRole("region", { name: "Needs attention" });
    expect(within(section).getByText(/Alien: access denied/)).toBeVisible();
    await userEvent.setup().click(within(section).getByRole("button", { name: "Open in Rename" }));
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({ to: "/rename", search: { ids: "4,9" } }),
    );
    // The preview is the retry, so the entry goes.
    expect(screen.queryByTestId("needs-attention")).toBeNull();
  });

  it("keeps unfinished moves when the warnings are dismissed", async () => {
    backend.batches = [batch("move-1")];
    useNotificationStore
      .getState()
      .add({ severity: "warning", title: "Artwork could not be saved" });
    renderSection();
    await userEvent.setup().click(await screen.findByRole("button", { name: "Dismiss all" }));
    expect(screen.getByRole("button", { name: "Resume" })).toBeVisible();
  });

  it("puts the focus on the batch that Open in Activity asked for", async () => {
    backend.batches = [batch("move-1"), batch("move-2", { destination: String.raw`G:\Films` })];
    renderSection("move-2");
    const row = (await screen.findByText(String.raw`Move to G:\Films did not finish`)).closest(
      "li",
    );
    await waitFor(() =>
      expect(document.activeElement).toBe(
        within(row as HTMLElement).getByRole("button", { name: "Resume" }),
      ),
    );
  });
});

describe("the start-up resume", () => {
  it("resumes only the moves the close cut short, once per window", async () => {
    backend.batches = [
      batch("move-1", { auto_resume: true }),
      batch("move-2", { reason: "Destination is full." }),
    ];
    await autoResumeUnfinished();
    expect(backend.resume).toHaveBeenCalledTimes(1);
    expect(backend.resume).toHaveBeenCalledWith("move-1", expect.any(Function));
    expect(toasts()).toContain("Resuming the move of Dune: Part Two");
    expect(toasts()).toContain("Finished the move of Dune: Part Two");

    backend.batches = [batch("move-3", { auto_resume: true })];
    await autoResumeUnfinished();
    expect(backend.resume).toHaveBeenCalledTimes(1);
  });

  it("waits while another move runs", async () => {
    backend.batches = [batch("move-1", { auto_resume: true })];
    useMoveToStore.setState({
      running: { label: "Heat", target: "E: Archive", percent: 10, paused: false },
    });
    await autoResumeUnfinished();
    expect(backend.resume).not.toHaveBeenCalled();
  });

  it("says when a resumed move stops again and leads to Activity", async () => {
    backend.batches = [batch("move-1", { auto_resume: true })];
    backend.resume = vi.fn((batchId: string) =>
      Promise.resolve({
        batch_id: batchId,
        completed: 0,
        stopped: "Destination is full",
        notes: [],
      }),
    );
    await autoResumeUnfinished();
    const stopped = useToastStore.getState().toasts.at(-1);
    expect(stopped?.message).toBe("The move of Dune: Part Two stopped again: Destination is full");
    expect(stopped?.action?.label).toBe("Open Activity");
  });
});
