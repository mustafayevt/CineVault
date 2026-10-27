import { beforeEach, describe, expect, it } from "vitest";

import { MAX_NOTIFICATIONS, notify, useNotificationStore } from "./notificationStore";

beforeEach(() => {
  useNotificationStore.setState({ items: [] });
});

describe("the message store behind Activity", () => {
  it("adds newest first", () => {
    notify({ severity: "error", title: "first" });
    notify({ severity: "warning", title: "second" });
    expect(useNotificationStore.getState().items.map((item) => item.title)).toEqual([
      "second",
      "first",
    ]);
  });

  it("merges the same message into one with a count and moves it to the top", () => {
    notify({ severity: "error", title: "Lookup failed", detail: "HTTP 503" });
    notify({ severity: "warning", title: "Slow provider" });
    notify({ severity: "error", title: "Lookup failed", detail: "HTTP 503" });
    notify({ severity: "error", title: "Lookup failed", detail: "HTTP 503" });
    const items = useNotificationStore.getState().items;
    expect(items.map((item) => item.title)).toEqual(["Lookup failed", "Slow provider"]);
    expect(items[0]?.repeats).toBe(3);
  });

  it("keeps messages that differ in their batch apart", () => {
    notify({ severity: "warning", title: "A move did not finish", batchId: "a" });
    notify({ severity: "warning", title: "A move did not finish", batchId: "b" });
    expect(useNotificationStore.getState().items).toHaveLength(2);
  });

  it("dismisses one message and clears all", () => {
    const first = notify({ severity: "error", title: "first" });
    notify({ severity: "error", title: "second" });
    useNotificationStore.getState().dismiss(first);
    expect(useNotificationStore.getState().items.map((item) => item.title)).toEqual(["second"]);
    useNotificationStore.getState().clear();
    expect(useNotificationStore.getState().items).toEqual([]);
  });

  it("keeps a bounded history", () => {
    for (let i = 0; i < MAX_NOTIFICATIONS + 20; i += 1) {
      notify({ severity: "info", title: `message ${i}` });
    }
    const items = useNotificationStore.getState().items;
    expect(items).toHaveLength(MAX_NOTIFICATIONS);
    expect(items[0]?.title).toBe(`message ${MAX_NOTIFICATIONS + 19}`);
  });
});
