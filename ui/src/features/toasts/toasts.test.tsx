import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { create } from "zustand";

import { TOAST_DURATION_MS, UNDO_TOAST_DURATION_MS } from "../../components/Toast";
import { useBottomInsetStore } from "../../stores/bottomInset";
import { useNotificationStore } from "../notifications/notificationStore";
import { MAX_TOASTS, showToast, useToastStore } from "./toastStore";
import { ToastViewport } from "./ToastViewport";
import { useUndoShortcut } from "./useUndoShortcut";

/** A stand-in for the library: which titles are watched, with the action that changes it. */
const useWatched = create<{
  watched: ReadonlySet<number>;
  markWatched: (ids: readonly number[]) => void;
}>((set, get) => ({
  watched: new Set<number>([1]),
  markWatched: (ids) => {
    const before = get().watched;
    set({ watched: new Set([...before, ...ids]) });
    showToast({
      message: `Marked ${ids.length} watched`,
      undo: () => set({ watched: before }),
    });
  },
}));

const IDS = Array.from({ length: 12 }, (_, index) => index);

function Harness() {
  useUndoShortcut("windows");
  return (
    <>
      <input aria-label="Notes" />
      <ToastViewport platform="windows" />
    </>
  );
}

beforeEach(() => {
  useBottomInsetStore.setState({ insets: {} });
  useToastStore.setState({ toasts: [], history: [] });
  useNotificationStore.setState({ items: [] });
  useWatched.setState({ watched: new Set([1]) });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("undo round trip", () => {
  it("confirms a bulk change with a toast and restores the previous state on Undo", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    act(() => useWatched.getState().markWatched(IDS));

    expect(useWatched.getState().watched.size).toBe(12);
    expect(screen.getByRole("status")).toHaveTextContent("Marked 12 watched");

    await user.click(screen.getByRole("button", { name: /^Undo/ }));

    expect([...useWatched.getState().watched]).toEqual([1]);
    // The undo is confirmed in a toast of its own, without Undo.
    expect(screen.getByRole("status")).toHaveTextContent("Undone: Marked 12 watched");
    expect(screen.queryByRole("button", { name: /^Undo/ })).toBeNull();
  });

  it("does not add a second confirmation when the undo says what it did", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    act(() => {
      showToast({
        message: "Edited 3 titles",
        undo: () => void showToast({ message: "Put back 3 titles" }),
      });
    });
    await user.click(screen.getByRole("button", { name: /^Undo/ }));
    expect(screen.getAllByRole("status").map((el) => el.textContent)).toEqual([
      "Put back 3 titles",
    ]);
  });

  it("keeps an action undoable with Ctrl Z after its toast has timed out", async () => {
    vi.useFakeTimers();
    render(<Harness />);
    act(() => useWatched.getState().markWatched(IDS));
    act(() => void vi.advanceTimersByTime(UNDO_TOAST_DURATION_MS + 1));
    expect(screen.queryByRole("status")).toBeNull();

    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true }));
      await Promise.resolve();
    });
    expect([...useWatched.getState().watched]).toEqual([1]);
    expect(screen.getByRole("status")).toHaveTextContent("Undone: Marked 12 watched");
  });

  it("undoes with the Ctrl Z shortcut and only while a toast offers Undo", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    act(() => useWatched.getState().markWatched(IDS));

    await user.keyboard("{Control>}z{/Control}");
    expect([...useWatched.getState().watched]).toEqual([1]);
    expect(screen.getByRole("status")).toHaveTextContent("Undone: Marked 12 watched");

    act(() => useWatched.setState({ watched: new Set([1, 2]) }));
    await user.keyboard("{Control>}z{/Control}");
    expect(useWatched.getState().watched.size).toBe(2);
  });

  it("runs an undo token once even when it is triggered twice", async () => {
    const undo = vi.fn();
    const id = showToast({ message: "Edited 3 titles", undo });
    const results = await Promise.all([
      useToastStore.getState().undo(id),
      useToastStore.getState().undo(id),
    ]);
    expect(undo).toHaveBeenCalledTimes(1);
    expect(results.filter(Boolean)).toHaveLength(1);
  });

  it("leaves Ctrl Z to a focused text field", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    act(() => useWatched.getState().markWatched(IDS));

    await user.click(screen.getByRole("textbox", { name: "Notes" }));
    await user.keyboard("{Control>}z{/Control}");

    expect(useWatched.getState().watched.size).toBe(12);
    expect(screen.getByRole("status")).toBeInTheDocument();
  });

  it("reports a failed undo to the notification centre without a dialog", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    act(() => {
      showToast({
        message: "Renamed 4 folders",
        undo: () => {
          throw new Error("Destination changed since the rename");
        },
      });
    });

    await user.click(screen.getByRole("button", { name: /^Undo/ }));

    const [entry] = useNotificationStore.getState().items;
    expect(entry).toMatchObject({
      severity: "error",
      title: "Could not undo: Renamed 4 folders",
      detail: "Destination changed since the rename",
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });
});

describe("toast stack", () => {
  it("shows a toast without Undo when the action has none", () => {
    render(<Harness />);
    act(() => void showToast({ message: "Saved 2 posters" }));
    expect(screen.getByRole("status")).toHaveTextContent("Saved 2 posters");
    expect(screen.queryByRole("button", { name: /^Undo/ })).toBeNull();
  });

  it("dismisses a toast after its time runs out", () => {
    vi.useFakeTimers();
    render(<Harness />);
    act(() => void showToast({ message: "Marked 1 watched", undo: () => {} }));
    act(() => void vi.advanceTimersByTime(UNDO_TOAST_DURATION_MS - 1));
    expect(screen.getByRole("status")).toBeInTheDocument();
    act(() => void vi.advanceTimersByTime(2));
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("keeps the newest toasts, lowest in the stack", () => {
    render(<Harness />);
    act(() => {
      for (let n = 1; n <= MAX_TOASTS + 1; n += 1) showToast({ message: `Action ${n}` });
    });
    const shown = screen.getAllByRole("status").map((el) => el.textContent);
    expect(shown).toHaveLength(MAX_TOASTS);
    expect(shown[0]).toContain("Action 2");
    expect(shown[MAX_TOASTS - 1]).toContain("Action 4");
  });

  it("shows the Undo shortcut", () => {
    render(<Harness />);
    act(() => void showToast({ message: "Marked 2 watched", undo: () => {} }));
    expect(screen.getByRole("button", { name: /^Undo/ })).toHaveTextContent("Ctrl Z");
  });
});

describe("toast placement", () => {
  it("stacks at the bottom right of the window, clear of the sidebar", () => {
    render(<ToastViewport platform="windows" />);
    const region = screen.getByRole("region", { name: "Toasts" });
    expect(region.className.split(" ")).toEqual(expect.arrayContaining(["fixed", "right-4"]));
    expect(region.className).not.toContain("left-");
  });
});

describe("merging, the cap and the offset", () => {
  it("merges identical messages into one toast with a count", () => {
    render(<Harness />);
    act(() => {
      for (let n = 0; n < 3; n += 1) showToast({ message: "Refreshed details for 7 episodes" });
    });
    const shown = screen.getAllByRole("status");
    expect(shown).toHaveLength(1);
    expect(shown[0]).toHaveTextContent("Refreshed details for 7 episodes×3");
    // A different message is a toast of its own.
    act(() => void showToast({ message: "Saved 2 posters" }));
    expect(screen.getAllByRole("status")).toHaveLength(2);
  });

  it("never merges toasts that offer Undo, so each action stays undoable", () => {
    render(<Harness />);
    act(() => {
      showToast({ message: "Marked 1 watched", undo: () => {} });
      showToast({ message: "Marked 1 watched", undo: () => {} });
    });
    expect(screen.getAllByRole("status")).toHaveLength(2);
    expect(useToastStore.getState().history).toHaveLength(2);
  });

  it("shows at most three toasts, dropping the oldest", () => {
    render(<Harness />);
    act(() => {
      for (let n = 1; n <= 5; n += 1) showToast({ message: `Step ${n}` });
    });
    expect(MAX_TOASTS).toBe(3);
    expect(screen.getAllByRole("status").map((el) => el.textContent)).toEqual([
      "Step 3",
      "Step 4",
      "Step 5",
    ]);
  });

  it("keeps Undo for about 8 seconds and a plain confirmation for less", () => {
    vi.useFakeTimers();
    render(<Harness />);
    act(() => {
      showToast({ message: "Plain" });
      showToast({ message: "With undo", undo: () => {} });
    });
    act(() => void vi.advanceTimersByTime(TOAST_DURATION_MS + 1));
    expect(screen.getAllByRole("status").map((el) => el.textContent)).toEqual([
      expect.stringContaining("With undo"),
    ]);
    act(() => void vi.advanceTimersByTime(UNDO_TOAST_DURATION_MS - TOAST_DURATION_MS));
    expect(screen.queryByRole("status")).toBeNull();
    expect(UNDO_TOAST_DURATION_MS).toBe(8000);
  });

  it("lifts the stack above a bar along the bottom and drops it when the bar goes", () => {
    render(<ToastViewport platform="windows" />);
    const region = screen.getByRole("region", { name: "Toasts" });
    expect(region.style.bottom).toBe("calc(var(--space-4) + 0px)");
    act(() => {
      useBottomInsetStore.getState().set("bulk-bar", 70);
      useBottomInsetStore.getState().set("workspace-bar", 96);
    });
    expect(region.style.bottom).toBe("calc(var(--space-4) + 96px)");
    act(() => useBottomInsetStore.getState().clear("workspace-bar"));
    expect(region.style.bottom).toBe("calc(var(--space-4) + 70px)");
    act(() => useBottomInsetStore.getState().clear("bulk-bar"));
    expect(region.style.bottom).toBe("calc(var(--space-4) + 0px)");
  });
});

describe("error toasts", () => {
  it("offer only the action their caller gives, never a pane that is gone", () => {
    render(<Harness />);
    act(() => void showToast({ message: "Could not scrape Heat", tone: "error" }));
    expect(screen.getByRole("status")).toHaveTextContent("Could not scrape Heat");
    expect(screen.queryByRole("button", { name: "View in Activity" })).toBeNull();
  });
});
