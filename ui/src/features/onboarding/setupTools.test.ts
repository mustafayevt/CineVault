import { afterEach, describe, expect, it, vi } from "vitest";

import { useToastStore } from "../toasts/toastStore";
import { setToolsBackend, unavailableBackend } from "../tools/toolsBackend";
import type { ToolStatus } from "../tools/toolsBackend";
import { downloadRecommendedTools, downloadTools, queueRecommendedTools } from "./setupTools";

function tool(id: string, state: ToolStatus["state"]): ToolStatus {
  return {
    id,
    name: id === "ffmpeg" ? "FFmpeg" : "MediaInfo",
    state,
    source: null,
    path: null,
    version: null,
    custom_path: null,
    download_version: null,
    download_bytes: null,
    update_available: false,
    downloaded: false,
    license: "",
    homepage: "",
  };
}

afterEach(() => {
  setToolsBackend(null);
  useToastStore.setState({ toasts: [], history: [] });
});

describe("downloading the recommended tools after the setup", () => {
  it("downloads the tools that are missing, one after the other, and skips the ready ones", async () => {
    const install = vi.fn<(id: string) => Promise<ToolStatus[]>>(() => Promise.resolve([]));
    setToolsBackend({
      ...unavailableBackend,
      available: true,
      list: () => Promise.resolve([tool("mediainfo", "not_installed"), tool("ffmpeg", "ready")]),
      install,
    });
    await downloadRecommendedTools();
    expect(install.mock.calls.map((call) => call[0])).toEqual(["mediainfo"]);
  });

  it("reports a failed download in a toast and goes on with the next tool", async () => {
    const install = vi
      .fn<(id: string) => Promise<ToolStatus[]>>()
      .mockRejectedValueOnce(new Error("No connection."))
      .mockResolvedValue([]);
    setToolsBackend({
      ...unavailableBackend,
      available: true,
      list: () =>
        Promise.resolve([tool("mediainfo", "not_installed"), tool("ffmpeg", "not_installed")]),
      install,
    });
    await downloadRecommendedTools();
    expect(install).toHaveBeenCalledTimes(2);
    const [entry] = useToastStore.getState().toasts;
    expect(entry?.message).toBe("MediaInfo could not be downloaded. No connection.");
    expect(entry?.tone).toBe("error");
  });

  it("does nothing where there is nothing to download", async () => {
    const install = vi.fn();
    setToolsBackend({ ...unavailableBackend, install });
    await downloadRecommendedTools();
    expect(install).not.toHaveBeenCalled();
  });

  it("adds the tools an answer asks for, and downloads a tool in both lists once", async () => {
    const install = vi.fn<(id: string) => Promise<ToolStatus[]>>(() => Promise.resolve([]));
    setToolsBackend({
      ...unavailableBackend,
      available: true,
      list: () =>
        Promise.resolve([
          tool("mediainfo", "not_installed"),
          tool("ffmpeg", "not_installed"),
          tool("indexer", "not_installed"),
        ]),
      install,
    });
    await downloadRecommendedTools(["indexer", "ffmpeg"]);
    expect(install.mock.calls.map((call) => call[0])).toEqual(["mediainfo", "ffmpeg", "indexer"]);
  });

  it("downloads only the extra tools when the recommended ones are left out", async () => {
    const install = vi.fn<(id: string) => Promise<ToolStatus[]>>(() => Promise.resolve([]));
    setToolsBackend({
      ...unavailableBackend,
      available: true,
      list: () =>
        Promise.resolve([
          tool("mediainfo", "not_installed"),
          tool("ffmpeg", "ready"),
          tool("indexer", "not_installed"),
        ]),
      install,
    });
    await downloadRecommendedTools(["indexer", "ffmpeg"], false);
    expect(install.mock.calls.map((call) => call[0])).toEqual(["indexer"]);
  });

  it("reports a failed extra tool and does not stop the setup", async () => {
    const install = vi
      .fn<(id: string) => Promise<ToolStatus[]>>()
      .mockRejectedValueOnce(new Error("No connection."))
      .mockResolvedValue([]);
    setToolsBackend({
      ...unavailableBackend,
      available: true,
      list: () =>
        Promise.resolve([tool("indexer", "not_installed"), tool("ffmpeg", "not_installed")]),
      install,
    });
    await expect(downloadRecommendedTools(["indexer", "ffmpeg"], false)).resolves.toBeUndefined();
    expect(install).toHaveBeenCalledTimes(2);
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });

  it("announces the missing tools before any download starts, so the first scan waits for them", async () => {
    const order: string[] = [];
    setToolsBackend({
      ...unavailableBackend,
      available: true,
      list: () =>
        Promise.resolve([
          tool("mediainfo", "ready"),
          tool("ffmpeg", "not_installed"),
          tool("indexer", "not_installed"),
        ]),
      install: (id) => {
        order.push(`install:${id}`);
        return Promise.resolve([]);
      },
      expect: (ids) => {
        order.push(`expect:${ids.join(",")}`);
        return Promise.resolve();
      },
    });
    const queued = await queueRecommendedTools(["indexer", "ffmpeg"]);
    expect(order).toEqual(["expect:ffmpeg,indexer"]);
    await downloadTools(queued);
    expect(order).toEqual(["expect:ffmpeg,indexer", "install:ffmpeg", "install:indexer"]);
  });
});
