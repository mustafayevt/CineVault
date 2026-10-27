import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useNotificationStore } from "../notifications/notificationStore";
import { useToastStore } from "../toasts/toastStore";
import { LibraryToolbar } from "../library/LibraryToolbar";
import { setMediaInfoBackend } from "./mediaInfoBackend";
import type { MediaInfoBackend, ReloadResult } from "./mediaInfoBackend";
import { ReloadMediaInfoButton } from "./ReloadMediaInfoButton";
import { reloadMediaInfo, useReloadStore } from "./reloadMediaInfo";
import { clearReloadScope, publishReloadScope } from "./reloadScope";

const ids = (count: number) => Array.from({ length: count }, (_, index) => String(index + 1));

function result(over: Partial<ReloadResult> = {}): ReloadResult {
  return {
    action: "reload-media-info",
    jobs: [1],
    status: "done",
    message: "Read the media info of 2 titles again; 0 could not be read.",
    titles: 2,
    done: 2,
    unchanged: 0,
    skipped: 0,
    failed: 0,
    left_out: [],
    lines: [],
    ...over,
  };
}

function backend(answer: () => Promise<ReloadResult> = () => Promise.resolve(result())) {
  const reload = vi.fn<MediaInfoBackend["reload"]>(() => answer());
  const spy: MediaInfoBackend = { reload };
  setMediaInfoBackend(spy);
  return reload;
}

const button = () => screen.getByRole("button", { name: "Reload media info" });

const toolbar = (type: "movies" | "collections") => (
  <LibraryToolbar
    type={type}
    counts={{}}
    onType={() => undefined}
    view="grid"
    onView={() => undefined}
    posterSize="m"
    onPosterSize={() => undefined}
    sort={[]}
    onSort={() => undefined}
    columns={[]}
    onColumn={() => undefined}
    scanning={false}
    onScan={() => undefined}
  />
);

beforeEach(() => {
  useToastStore.setState({ toasts: [], history: [] });
  useNotificationStore.setState({ items: [] });
  useReloadStore.setState({ running: false });
});
afterEach(() => {
  setMediaInfoBackend(null);
  clearReloadScope();
});

describe("the Reload media info button", () => {
  it("is not there while the toolbar shows no titles", () => {
    render(<ReloadMediaInfoButton />);
    expect(screen.queryByRole("button", { name: "Reload media info" })).toBeNull();
  });

  it("with nothing selected asks first, then reloads every title of the list", async () => {
    const reload = backend();
    publishReloadScope({ type: "movies", selected: [], all: ids(180) });
    const user = userEvent.setup();
    render(<ReloadMediaInfoButton />);
    await user.click(button());
    expect(
      await screen.findByText(
        "Reload media info for 180 movies? This reads every file again and can take a few minutes.",
      ),
    ).toBeVisible();
    expect(reload).not.toHaveBeenCalled();
    // Cancel leaves everything as it was.
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(reload).not.toHaveBeenCalled();
    await user.click(button());
    await user.click(await screen.findByRole("button", { name: "Reload" }));
    await waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
    expect(reload.mock.calls[0]?.[0]).toHaveLength(180);
  });

  it("says TV shows for the shows list", async () => {
    backend();
    publishReloadScope({ type: "tv", selected: [], all: ids(23) });
    const user = userEvent.setup();
    render(<ReloadMediaInfoButton />);
    await user.click(button());
    expect(await screen.findByText(/^Reload media info for 23 TV shows\?/)).toBeVisible();
  });

  it("with titles selected reloads just those, with no question", async () => {
    const reload = backend();
    publishReloadScope({ type: "movies", selected: ["4", "9"], all: ids(180) });
    const user = userEvent.setup();
    render(<ReloadMediaInfoButton />);
    await user.click(button());
    await waitFor(() => expect(reload).toHaveBeenCalledWith(["4", "9"]));
    expect(screen.queryByRole("button", { name: "Reload" })).toBeNull();
  });

  it("stays off while a reload is running and when the list is empty", async () => {
    backend(() => new Promise<ReloadResult>(() => undefined));
    publishReloadScope({ type: "movies", selected: ["1"], all: ids(5) });
    const user = userEvent.setup();
    render(<ReloadMediaInfoButton />);
    await user.click(button());
    await waitFor(() => expect(button()).toBeDisabled());
    act(() => {
      useReloadStore.setState({ running: false });
      publishReloadScope({ type: "movies", selected: [], all: [] });
    });
    expect(button()).toBeDisabled();
  });

  it("sits right before the Scan button of the toolbar", () => {
    publishReloadScope({ type: "movies", selected: [], all: ids(3) });
    render(toolbar("movies"));
    const buttons = screen.getAllByRole("button").map((b) => b.getAttribute("aria-label"));
    const at = buttons.indexOf("Reload media info");
    expect(at).toBeGreaterThan(-1);
    expect(buttons[at + 1]).toBe("Scan folders now");
  });

  it("is not in the toolbar of the Collections view", () => {
    publishReloadScope({ type: "movies", selected: [], all: ids(3) });
    render(toolbar("collections"));
    expect(screen.queryByRole("button", { name: "Reload media info" })).toBeNull();
  });
});

describe("reloading media info", () => {
  it("says it started, and how it ended", async () => {
    backend();
    await reloadMediaInfo(["1", "2"]);
    const messages = useToastStore.getState().toasts.map((toast) => toast.message);
    expect(messages).toContain("Reloading media info for 2 titles…");
    expect(messages).toContain("Read the media info of 2 titles again; 0 could not be read.");
    expect(useReloadStore.getState().running).toBe(false);
  });

  it("lists the titles that could not be read again in the notification centre", async () => {
    backend(() =>
      Promise.resolve(
        result({
          failed: 1,
          lines: [{ title: "Heat", status: "failed", message: "file missing" }],
        }),
      ),
    );
    await reloadMediaInfo(["1"]);
    const [entry] = useNotificationStore.getState().items;
    expect(entry?.title).toBe("1 title could not be read again");
    expect(entry?.detail).toBe("Heat: file missing");
  });

  it("reports a missing MediaInfo or a refusal in the notification centre, never a dialog", async () => {
    backend(() =>
      Promise.resolve(
        result({ status: "unavailable", message: "MediaInfo is not installed.", done: 0 }),
      ),
    );
    await reloadMediaInfo(["1"]);
    expect(useNotificationStore.getState().items[0]?.title).toBe("MediaInfo is not installed.");
    setMediaInfoBackend({ reload: () => Promise.reject(new Error("boom")) });
    await reloadMediaInfo(["1"]);
    expect(useNotificationStore.getState().items[0]?.title).toBe(
      "Could not reload media info: boom",
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(useReloadStore.getState().running).toBe(false);
  });

  it("does nothing for no titles, and starts one reload at a time", async () => {
    const reload = backend(() => new Promise<ReloadResult>(() => undefined));
    await reloadMediaInfo([]);
    expect(reload).not.toHaveBeenCalled();
    void reloadMediaInfo(["1"]);
    void reloadMediaInfo(["2"]);
    expect(reload).toHaveBeenCalledTimes(1);
  });
});
