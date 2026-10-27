import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({ tauri: true, setZoom: vi.fn(() => Promise.resolve()) }));

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => native.tauri }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ setZoom: native.setZoom }),
}));

import { BOOT_LOOK_KEY } from "./bootLook";
import { settingDef } from "./schema";
import { useSettingsStore } from "./settingsStore";
import {
  ZOOM_STEPS,
  applyBootZoom,
  applyNativeZoom,
  nextZoom,
  snapZoom,
  stepAppZoom,
  zoomActionForKey,
} from "./zoom";

beforeEach(() => {
  native.tauri = true;
  native.setZoom.mockClear();
  useSettingsStore.setState({ values: {}, status: "ready" });
});

afterEach(() => {
  window.localStorage.removeItem(BOOT_LOOK_KEY);
});

describe("the zoom steps", () => {
  it("are 80, 90, 100, 110, 125 and 150 percent, with 100 as the default", () => {
    expect(ZOOM_STEPS).toEqual([80, 90, 100, 110, 125, 150]);
    expect(settingDef("appearance.zoom")?.default).toBe(100);
  });

  it("zoom in and out one step at a time and stop at both ends", () => {
    expect(nextZoom(100, "in")).toBe(110);
    expect(nextZoom(110, "in")).toBe(125);
    expect(nextZoom(125, "in")).toBe(150);
    expect(nextZoom(150, "in")).toBe(150);
    expect(nextZoom(100, "out")).toBe(90);
    expect(nextZoom(90, "out")).toBe(80);
    expect(nextZoom(80, "out")).toBe(80);
  });

  it("reset to 100 from anywhere", () => {
    expect(nextZoom(150, "reset")).toBe(100);
    expect(nextZoom(80, "reset")).toBe(100);
  });

  it("snap a value that is not a step to the nearest step", () => {
    expect(snapZoom(104)).toBe(100);
    expect(snapZoom(118)).toBe(125);
    expect(snapZoom(500)).toBe(150);
    expect(snapZoom(Number.NaN)).toBe(100);
    expect(nextZoom(104, "in")).toBe(110);
  });
});

describe("the zoom keys", () => {
  const ctrl = { ctrlKey: true, metaKey: false, altKey: false, code: "" };

  it("Ctrl with plus, equals or the keypad's plus zoom in", () => {
    expect(zoomActionForKey({ ...ctrl, key: "=" }, false)).toBe("in");
    expect(zoomActionForKey({ ...ctrl, key: "+" }, false)).toBe("in");
    expect(zoomActionForKey({ ...ctrl, key: "+", code: "NumpadAdd" }, false)).toBe("in");
  });

  it("Ctrl with minus or the keypad's minus zoom out, and Ctrl+0 resets", () => {
    expect(zoomActionForKey({ ...ctrl, key: "-" }, false)).toBe("out");
    expect(zoomActionForKey({ ...ctrl, key: "-", code: "NumpadSubtract" }, false)).toBe("out");
    expect(zoomActionForKey({ ...ctrl, key: "0" }, false)).toBe("reset");
  });

  it("use Command on macOS and Ctrl elsewhere", () => {
    const command = { ctrlKey: false, metaKey: true, altKey: false, code: "" };
    expect(zoomActionForKey({ ...command, key: "=" }, true)).toBe("in");
    expect(zoomActionForKey({ ...ctrl, key: "=" }, true)).toBeNull();
    expect(zoomActionForKey({ ...command, key: "=" }, false)).toBeNull();
  });

  it("ignore keys without the modifier, Alt chords and other keys", () => {
    expect(zoomActionForKey({ ...ctrl, ctrlKey: false, key: "=" }, false)).toBeNull();
    expect(zoomActionForKey({ ...ctrl, altKey: true, key: "=" }, false)).toBeNull();
    expect(zoomActionForKey({ ...ctrl, key: "a" }, false)).toBeNull();
  });
});

describe("changing the zoom", () => {
  it("steps the setting and clears it at 100", () => {
    stepAppZoom("in");
    expect(useSettingsStore.getState().values["appearance.zoom"]).toBe(110);
    stepAppZoom("in");
    stepAppZoom("in");
    stepAppZoom("in");
    expect(useSettingsStore.getState().values["appearance.zoom"]).toBe(150);
    stepAppZoom("reset");
    expect(useSettingsStore.getState().values["appearance.zoom"]).toBeUndefined();
  });

  it("stays on the last step instead of going further", () => {
    useSettingsStore.setState({ values: { "appearance.zoom": 80 } });
    stepAppZoom("out");
    expect(useSettingsStore.getState().values["appearance.zoom"]).toBe(80);
  });

  it("zooms the webview by a factor", async () => {
    await applyNativeZoom(125);
    expect(native.setZoom).toHaveBeenCalledWith(1.25);
  });

  it("does nothing, without an error, in a plain browser", async () => {
    native.tauri = false;
    await applyNativeZoom(125);
    expect(native.setZoom).not.toHaveBeenCalled();
  });

  it("puts back the last session's zoom on start", () => {
    window.localStorage.setItem(
      BOOT_LOOK_KEY,
      JSON.stringify({ theme: "dark", motion: "system", zoom: 110 }),
    );
    applyBootZoom();
    expect(native.setZoom).toHaveBeenCalledWith(1.1);
  });

  it("leaves a look saved before the zoom existed alone", () => {
    window.localStorage.setItem(BOOT_LOOK_KEY, JSON.stringify({ theme: "dark", motion: "system" }));
    applyBootZoom();
    expect(native.setZoom).not.toHaveBeenCalled();
  });
});
