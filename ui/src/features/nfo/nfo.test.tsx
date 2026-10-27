import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SETTINGS, settingDef } from "../settings/schema";
import { useNotificationStore } from "../notifications/notificationStore";
import { useToastStore } from "../toasts/toastStore";
import { ToastViewport } from "../toasts/ToastViewport";
import { runNfoAction, selectionScope } from "./nfoActions";
import { NfoRequestError, setNfoBackend } from "./nfoBackend";
import type { NfoActionResult, NfoBackend, NfoUndoResult } from "./nfoBackend";

function result(overrides: Partial<NfoActionResult> = {}): NfoActionResult {
  return {
    kind: "rewrite",
    token: null,
    changed: 0,
    unchanged: 0,
    kept_foreign: 0,
    skipped: 0,
    failed: 0,
    files_written: 0,
    files_trashed: 0,
    notes: [],
    ...overrides,
  };
}

interface FakeBackend extends NfoBackend {
  read: ReturnType<typeof vi.fn<NfoBackend["read"]>>;
  rewrite: ReturnType<typeof vi.fn<NfoBackend["rewrite"]>>;
  undo: ReturnType<typeof vi.fn<NfoBackend["undo"]>>;
}

function fakeBackend(): FakeBackend {
  return {
    available: true,
    read: vi.fn<NfoBackend["read"]>(),
    rewrite: vi.fn<NfoBackend["rewrite"]>(),
    undo: vi.fn<NfoBackend["undo"]>(),
  };
}

let backend: FakeBackend;

beforeEach(() => {
  backend = fakeBackend();
  setNfoBackend(backend);
  useToastStore.setState({ toasts: [] });
  useNotificationStore.setState({ items: [] });
});

afterEach(() => {
  setNfoBackend(null);
});

describe("Read NFO and Rewrite NFO actions", () => {
  it("confirms a rewrite with an Undo toast, and Undo reverses it by token", async () => {
    const user = userEvent.setup();
    backend.rewrite.mockResolvedValue(result({ changed: 2, token: "nfo-1-0" }));
    const undone: NfoUndoResult = { restored: 3, skipped: [] };
    backend.undo.mockResolvedValue(undone);
    render(<ToastViewport platform="windows" />);

    await act(() => runNfoAction("rewrite", selectionScope(["4", "7"])));

    expect(backend.rewrite).toHaveBeenCalledWith({ kind: "selection", ids: ["4", "7"] });
    expect(screen.getByRole("status")).toHaveTextContent("Rewrote NFO files for 2 titles");

    await user.click(screen.getByRole("button", { name: /^Undo/ }));
    expect(backend.undo).toHaveBeenCalledWith("nfo-1-0");
    expect(await screen.findByText("Undid the NFO changes (3 restored)")).toBeInTheDocument();
  });

  it("confirms a read with the number of titles and offers Undo", async () => {
    backend.read.mockResolvedValue(result({ kind: "read", changed: 1, token: "nfo-2-0" }));
    await act(() => runNfoAction("read", { kind: "selection", ids: ["1"] }));
    const toast = useToastStore.getState().toasts[0];
    expect(toast?.message).toBe("Read NFO files into 1 title");
    expect(toast?.undo).toBeDefined();
  });

  it("says so, without an Undo, when nothing needed to change", async () => {
    backend.rewrite.mockResolvedValue(result({ unchanged: 5 }));
    await act(() => runNfoAction("rewrite", { kind: "everything" }));
    const toast = useToastStore.getState().toasts[0];
    expect(toast?.message).toBe("The NFO files are already up to date");
    expect(toast?.undo).toBeUndefined();
    expect(useNotificationStore.getState().items).toHaveLength(0);
  });

  it("lists titles that were left out, and files that are not CineVault's, in a notification", async () => {
    backend.rewrite.mockResolvedValue(
      result({
        changed: 1,
        token: "nfo-3-0",
        kept_foreign: 1,
        skipped: 1,
        notes: [
          { item: "4", label: "Heat", code: "kept_foreign", detail: "" },
          { item: "9", label: "Ronin", code: "skipped_folder_missing", detail: "" },
        ],
      }),
    );
    await act(() => runNfoAction("rewrite", { kind: "everything" }));
    const [note] = useNotificationStore.getState().items;
    expect(note?.severity).toBe("warning");
    expect(note?.detail).toContain(
      "Heat: A file CineVault did not write is in the way and was left untouched",
    );
    expect(note?.detail).toContain("Ronin: The folder is not reachable");
  });

  it("reports a failure in the notification centre, not as a toast", async () => {
    backend.read.mockRejectedValue(new NfoRequestError("storage", "The library is locked."));
    await act(() => runNfoAction("read", { kind: "everything" }));
    expect(useToastStore.getState().toasts).toHaveLength(0);
    const [note] = useNotificationStore.getState().items;
    expect(note?.severity).toBe("error");
    expect(note?.title).toContain("The library is locked.");
  });

  it("warns when an undo had to leave something as it was", async () => {
    backend.rewrite.mockResolvedValue(result({ changed: 1, token: "nfo-4-0" }));
    backend.undo.mockResolvedValue({ restored: 0, skipped: ["Heat.nfo: the file changed"] });
    await act(() => runNfoAction("rewrite", { kind: "everything" }));
    const toast = useToastStore.getState().toasts[0];
    await act(() => useToastStore.getState().undo(toast?.id ?? ""));
    const [note] = useNotificationStore.getState().items;
    expect(note?.severity).toBe("warning");
    expect(note?.detail).toBe("Heat.nfo: the file changed");
  });
});

describe("NFO settings", () => {
  // The Rust side (`cv_app::nfo::NfoSettings`) reads these keys with these defaults.
  const DEFAULTS: Record<string, string | boolean> = {
    "metadata.nfo-format": "kodi",
    "metadata.nfo-mediaportal-style": "legacy",
    "metadata.nfo-movie-names": "video-base",
    "metadata.nfo-set-names": "set-name",
    "metadata.nfo-season-names": "season",
    "metadata.nfo-disc-folder": false,
    "metadata.nfo-collection-folder": "",
    "metadata.nfo-collection-tmdb-id": false,
    "metadata.nfo-language": "en",
    "metadata.nfo-certification": "short",
    "metadata.nfo-create-outline": false,
    "metadata.nfo-outline-first-sentence": true,
    "metadata.nfo-single-studio": false,
    "metadata.nfo-date-added": true,
    "metadata.nfo-date-added-source": "added",
    "metadata.nfo-artwork-urls": true,
    "metadata.nfo-trailer": true,
    "metadata.nfo-fileinfo": true,
    "metadata.nfo-external-audio": false,
    "metadata.nfo-lockdata": false,
    "metadata.nfo-all-actors": false,
    "metadata.nfo-episodeguide": "json",
    "metadata.nfo-end-date": true,
    "metadata.nfo-clean": false,
  };

  it("lists every NFO option with the default the backend assumes", () => {
    for (const [id, value] of Object.entries(DEFAULTS)) {
      expect(settingDef(id)?.default, id).toBe(value);
    }
    const nfoIds = SETTINGS.filter((s) => s.id.startsWith("metadata.nfo-")).map((s) => s.id);
    expect([...nfoIds].sort()).toEqual(Object.keys(DEFAULTS).sort());
  });

  it("offers every format and naming choice the backend understands", () => {
    const values = (id: string): string[] => {
      const control = settingDef(id)?.control;
      return control?.type === "choice" ? control.options.map((o) => o.value) : [];
    };
    expect(values("metadata.nfo-format")).toEqual([
      "kodi",
      "jellyfin",
      "emby",
      "xbmc",
      "plex",
      "mediaportal",
    ]);
    expect(values("metadata.nfo-movie-names")).toEqual(["video-base", "movie", "both"]);
    expect(values("metadata.nfo-set-names")).toEqual(["set-name", "set", "collection", "all"]);
    expect(values("metadata.nfo-season-names")).toEqual(["season", "season-folder", "both"]);
    expect(values("metadata.nfo-episodeguide")).toEqual(["json", "url", "off"]);
    expect(values("metadata.nfo-date-added-source")).toEqual([
      "added",
      "file-created",
      "file-modified",
      "release",
    ]);
    expect(values("metadata.nfo-certification")).toHaveLength(8);
  });
});
