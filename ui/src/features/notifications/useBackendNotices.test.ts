import { beforeEach, describe, expect, it, vi } from "vitest";

import type { NoticeDto } from "../../bindings";

/** Stand-in for the Tauri channel: the test pushes messages through `onmessage`. */
const { FakeChannel } = vi.hoisted(() => ({
  FakeChannel: class FakeChannel<T> {
    onmessage: (message: T) => void = () => {};
  },
}));

vi.mock("@tauri-apps/api/core", () => ({ Channel: FakeChannel }));

vi.mock("../../bindings", () => ({
  commands: { subscribeNotices: vi.fn() },
}));

import { commands } from "../../bindings";
import { useNotificationStore } from "./notificationStore";
import { openNoticeStream, resetNoticeStream } from "./useBackendNotices";

const subscribe = vi.mocked(commands.subscribeNotices);

function lastChannel(): InstanceType<typeof FakeChannel<NoticeDto>> {
  const call = subscribe.mock.calls.at(-1);
  if (!call) throw new Error("not subscribed");
  return call[0] as unknown as InstanceType<typeof FakeChannel<NoticeDto>>;
}

describe("backend notices", () => {
  beforeEach(() => {
    resetNoticeStream();
    useNotificationStore.getState().clear();
    subscribe.mockReset();
    subscribe.mockResolvedValue(undefined);
  });

  it("keeps what the backend sends for Activity", () => {
    openNoticeStream();
    lastChannel().onmessage({
      severity: "warning",
      title: "Scraping 3 titles: 3 items failed",
      detail: "3 failed · rate limit",
      source: "tmdb",
      provider_key: null,
      settings_link: null,
      action: null,
      batch_id: null,
      topic: null,
      resolves: false,
    });
    const [item] = useNotificationStore.getState().items;
    expect(item).toMatchObject({
      severity: "warning",
      title: "Scraping 3 titles: 3 items failed",
      detail: "3 failed · rate limit",
      source: "tmdb",
    });
  });

  it("leaves out detail and source the backend did not send", () => {
    openNoticeStream();
    lastChannel().onmessage({
      severity: "error",
      title: "Backup failed",
      detail: null,
      source: null,
      provider_key: null,
      settings_link: null,
      action: null,
      batch_id: null,
      topic: null,
      resolves: false,
    });
    const [item] = useNotificationStore.getState().items;
    expect(item?.detail).toBeUndefined();
    expect(item?.source).toBeUndefined();
    expect(item?.providerKey).toBeUndefined();
  });

  it("keeps the provider whose key was rejected, so the message can link to its card", () => {
    openNoticeStream();
    lastChannel().onmessage({
      severity: "error",
      title:
        "TMDB rejected your key: Invalid API key. Check it in Settings, Scraping and providers.",
      detail: null,
      source: "tmdb",
      provider_key: "tmdb",
      settings_link: null,
      action: null,
      batch_id: null,
      topic: null,
      resolves: false,
    });
    const [item] = useNotificationStore.getState().items;
    expect(item?.providerKey).toBe("tmdb");
  });

  it("keeps the fix a notice offers and the unfinished batch it is about", () => {
    openNoticeStream();
    lastChannel().onmessage({
      severity: "warning",
      title: "A move to another source did not finish",
      detail: null,
      source: "Recovery",
      provider_key: null,
      settings_link: null,
      action: "resolve_unfinished",
      batch_id: "b1",
      topic: null,
      resolves: false,
    });
    lastChannel().onmessage({
      severity: "warning",
      title: "IMDb ratings could not be fetched",
      detail: null,
      source: null,
      provider_key: null,
      settings_link: null,
      action: "retry_imdb_dataset",
      batch_id: null,
      topic: null,
      resolves: false,
    });
    const [imdb, move] = useNotificationStore.getState().items;
    expect(move).toMatchObject({ action: "resolve_unfinished", batchId: "b1" });
    expect(imdb?.action).toBe("retry_imdb_dataset");
    expect(imdb?.batchId).toBeUndefined();
  });

  it("replaces a folder's warning by its next one and drops it once it no longer holds (final-13)", () => {
    openNoticeStream();
    const notice = (title: string, resolves = false): NoticeDto => ({
      severity: "warning",
      title,
      detail: null,
      source: "Library scan",
      provider_key: null,
      settings_link: null,
      action: null,
      batch_id: null,
      topic: "source-skipped:E:\\MoviesE",
      resolves,
    });
    lastChannel().onmessage(notice("MoviesE could not be read"));
    lastChannel().onmessage(notice("NAS 4K Shelf could not be read"));
    expect(useNotificationStore.getState().items.map((item) => item.title)).toEqual([
      "NAS 4K Shelf could not be read",
    ]);
    lastChannel().onmessage(notice("", true));
    expect(useNotificationStore.getState().items).toEqual([]);
  });

  it("opens the stream once however often the shell mounts", () => {
    openNoticeStream();
    openNoticeStream();
    expect(subscribe).toHaveBeenCalledTimes(1);
  });

  it("tries again after the backend refused the subscription", async () => {
    subscribe.mockRejectedValueOnce(new Error("no shell"));
    openNoticeStream();
    await Promise.resolve();
    await Promise.resolve();
    openNoticeStream();
    expect(subscribe).toHaveBeenCalledTimes(2);
  });
});
