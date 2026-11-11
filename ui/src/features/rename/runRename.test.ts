import { QueryClient } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type {
  AppliedDto,
  AppliedItemDto,
  OrganizeRequestDto,
  RenameProgressDto,
} from "../../bindings";
import { useNotificationStore } from "../notifications/notificationStore";
import { useToastStore } from "../toasts/toastStore";
import { setOrganizeBackend } from "./organizeBackend";
import type { OrganizeBackend } from "./organizeBackend";
import { OPEN_RENAME_ACTION, runRename, useRenameRunStore } from "./runRename";
import type { RunningRename } from "./runRename";
import { syntheticOrganizeBackend } from "./syntheticOrganize";

const REQUEST: OrganizeRequestDto = {
  scope: { kind: "everything" },
  profile_id: "default",
  patterns: {
    movie_folder: "${title} (${year})",
    movie_file: "${title} (${year})",
    show_folder: "${showTitle}",
    season_folder: "Season ${seasonNr2}",
    episode_file: "${showTitle} - S${seasonNr2}E${episodeNr2}",
  },
  choices: [],
};

function item(id: string, failed: boolean): AppliedItemDto {
  return {
    id,
    title: `Title ${id}`,
    status: failed ? "failed" : "done",
    error: failed ? "the file is in use by another program" : "",
    moved: 1,
    copied: 0,
    trashed: 0,
    notes: [],
  };
}

/** A backend whose apply reports each title, then ends with `failed` of the ids failed. */
function backendWith(failed: readonly string[], seen: RunningRename[]): OrganizeBackend {
  return {
    ...syntheticOrganizeBackend,
    apply: (_request, include, onProgress) => {
      const total = include.length;
      include.forEach((id, index) => {
        const progress: RenameProgressDto = { done: index + 1, total, title: `Title ${id}` };
        onProgress?.(progress);
        const running = useRenameRunStore.getState().running;
        if (running !== null) seen.push(running);
      });
      const items = include.map((id) => item(id, failed.includes(id)));
      const applied: AppliedDto = {
        batch_id: items.some((entry) => entry.status === "done") || failed.length > 0 ? "b1" : null,
        done: items.filter((entry) => entry.status === "done").length,
        failed: failed.length,
        items,
        not_run: [],
      };
      return Promise.resolve(applied);
    },
  };
}

const client = () => new QueryClient();
const messages = () => useToastStore.getState().toasts.map((toast) => toast.message);

beforeEach(() => {
  useToastStore.setState({ toasts: [], history: [] });
  useNotificationStore.setState({ items: [] });
  useRenameRunStore.setState({ running: null });
});

afterEach(() => {
  setOrganizeBackend(null);
});

describe("a rename that runs outside the page", () => {
  it("shows its progress while it runs and says Renamed with Undo when it ends", async () => {
    const seen: RunningRename[] = [];
    setOrganizeBackend(backendWith([], seen));
    const ids = Array.from({ length: 12 }, (_, n) => String(n + 1));
    await runRename(REQUEST, ids, client());
    expect(seen.at(2)).toEqual({
      name: "Renaming 12 titles",
      done: 3,
      total: 12,
      title: "Title 3",
    });
    expect(useRenameRunStore.getState().running).toBeNull();
    expect(messages()).toEqual(["Renamed 12 titles"]);
    expect(useToastStore.getState().toasts[0]?.undo).toBeDefined();
    expect(useNotificationStore.getState().items).toHaveLength(0);
  });

  it("says how many need attention and lists them there with Open in Rename", async () => {
    setOrganizeBackend(backendWith(["4", "9"], []));
    const ids = Array.from({ length: 12 }, (_, n) => String(n + 1));
    await runRename(REQUEST, ids, client());
    const toast = useToastStore.getState().toasts[0];
    expect(toast?.message).toBe("Renamed 10 of 12 titles; 2 need attention");
    expect(toast?.undo).toBeDefined();
    expect(toast?.action?.label).toBe("Open Activity");
    const notice = useNotificationStore.getState().items[0];
    expect(notice?.severity).toBe("error");
    expect(notice?.title).toBe("2 titles could not be renamed");
    expect(notice?.detail).toBe(
      "Title 4: the file is in use by another program\nTitle 9: the file is in use by another program",
    );
    expect(notice?.action).toBe(OPEN_RENAME_ACTION);
    expect(notice?.itemIds).toEqual(["4", "9"]);
  });

  it("is an error with Open Activity when nothing could be renamed", async () => {
    setOrganizeBackend(backendWith(["1", "2"], []));
    await runRename(REQUEST, ["1", "2"], client());
    const toast = useToastStore.getState().toasts[0];
    expect(toast?.message).toBe("2 titles could not be renamed");
    expect(toast?.tone).toBe("error");
    expect(toast?.undo).toBeUndefined();
  });

  it("runs one rename at a time", async () => {
    useRenameRunStore.setState({
      running: { name: "Renaming 2 titles", done: 0, total: 2, title: "" },
    });
    expect(await runRename(REQUEST, ["1"], client())).toBe(false);
  });
});
