import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({ setZoom: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ setZoom: native.setZoom }),
}));

import { applyAppearance, resolveMotion, useAppearance } from "./appearance";
import { settingDef } from "./schema";
import { useSettingsStore } from "./settingsStore";
import { BOOT_LOOK_KEY, readBootLook } from "./bootLook";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

afterEach(() => {
  native.setZoom.mockClear();
  const root = document.documentElement;
  delete root.dataset.motion;
  window.localStorage.removeItem(BOOT_LOOK_KEY);
});

describe("the look on start", () => {
  afterEach(() => {
    useSettingsStore.setState({ values: {}, status: "idle" });
  });

  it("draws the last session's look until the settings are read, then the settings, and keeps them", () => {
    window.localStorage.setItem(
      BOOT_LOOK_KEY,
      JSON.stringify({ theme: "light", motion: "system" }),
    );
    useSettingsStore.setState({ values: {}, status: "loading" });
    render(<Probe />);
    const root = document.documentElement;
    // Not the default (dark) while the settings file is still being read.
    expect(root.dataset.theme).toBe("light");

    act(() =>
      useSettingsStore.setState({
        values: { "appearance.theme": "dark", "appearance.motion": "reduced" },
        status: "ready",
      }),
    );
    expect(root.dataset.theme).toBe("dark");
    expect(root.dataset.motion).toBe("reduced");
    // The next start draws this look before the settings are read.
    expect(readBootLook()).toMatchObject({ theme: "dark", motion: "reduced" });
  });

  it("uses the settings when there is no saved look (a first start)", () => {
    useSettingsStore.setState({ values: { "appearance.theme": "light" }, status: "loading" });
    render(<Probe />);
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("reads the saved look in index.html before the first paint", () => {
    const html = readFileSync(resolve(__dirname, "../../../index.html"), "utf8");
    expect(html).toContain('localStorage.getItem("' + BOOT_LOOK_KEY + '")');
  });

  it("ignores a saved look it cannot read", () => {
    window.localStorage.setItem(BOOT_LOOK_KEY, "{not json");
    expect(readBootLook()).toBeNull();
    window.localStorage.setItem(BOOT_LOOK_KEY, JSON.stringify({ theme: "pink" }));
    expect(readBootLook()).toBeNull();
  });
});

function Probe() {
  useAppearance();
  return <p>probe</p>;
}

describe("reduced-motion switch", () => {
  it("defaults to following the system", () => {
    expect(settingDef("appearance.motion")?.default).toBe("system");
  });

  it("sets data-motion to reduced or full, and leaves it off for System without a system preference", () => {
    const root = document.documentElement;
    applyAppearance(root, { theme: "dark", motion: "reduced" });
    expect(root.dataset.motion).toBe("reduced");
    applyAppearance(root, { theme: "dark", motion: "full" });
    expect(root.dataset.motion).toBe("full");
    applyAppearance(root, { theme: "dark", motion: "system" });
    expect(root.dataset.motion).toBeUndefined();
  });

  it("sets data-motion=reduced when System follows an operating system that asks for it", () => {
    const root = document.documentElement;
    const look = { theme: "dark", motion: "system" } as const;
    applyAppearance(root, { ...look, systemReducedMotion: true });
    expect(root.dataset.motion).toBe("reduced");
    // Full forces motion on even then; Reduced stays reduced without it.
    applyAppearance(root, { ...look, motion: "full", systemReducedMotion: true });
    expect(root.dataset.motion).toBe("full");
    applyAppearance(root, { ...look, motion: "reduced", systemReducedMotion: false });
    expect(root.dataset.motion).toBe("reduced");
    expect(resolveMotion("system", false)).toBeNull();
  });

  it("applies the Motion setting live", () => {
    render(<Probe />);
    expect(screen.getByText("probe")).toBeInTheDocument();
    expect(document.documentElement.dataset.motion).toBeUndefined();
    act(() => useSettingsStore.getState().set("appearance.motion", "reduced"));
    expect(document.documentElement.dataset.motion).toBe("reduced");
    // Settings offers System and Reduced only; a value from before that is read as System.
    act(() => useSettingsStore.getState().set("appearance.motion", "full"));
    expect(document.documentElement.dataset.motion).toBeUndefined();
  });
});

describe("the app zoom", () => {
  afterEach(() => {
    useSettingsStore.setState({ values: {}, status: "idle" });
  });

  it("zooms the webview with the setting, live", () => {
    useSettingsStore.setState({ values: {}, status: "ready" });
    render(<Probe />);
    expect(native.setZoom).toHaveBeenLastCalledWith(1);
    act(() => useSettingsStore.getState().set("appearance.zoom", 125));
    expect(native.setZoom).toHaveBeenLastCalledWith(1.25);
    act(() => useSettingsStore.getState().set("appearance.zoom", 100));
    expect(native.setZoom).toHaveBeenLastCalledWith(1);
  });

  it("uses the last session's zoom until the settings are read, then the settings, and keeps it", () => {
    window.localStorage.setItem(
      BOOT_LOOK_KEY,
      JSON.stringify({ theme: "dark", motion: "system", zoom: 110 }),
    );
    useSettingsStore.setState({ values: {}, status: "loading" });
    render(<Probe />);
    expect(native.setZoom).toHaveBeenLastCalledWith(1.1);
    act(() => useSettingsStore.setState({ values: { "appearance.zoom": 90 }, status: "ready" }));
    expect(native.setZoom).toHaveBeenLastCalledWith(0.9);
    expect(readBootLook()).toMatchObject({ zoom: 90 });
  });

  it("accepts a saved look from before the zoom existed", () => {
    window.localStorage.setItem(
      BOOT_LOOK_KEY,
      JSON.stringify({ theme: "light", motion: "system" }),
    );
    expect(readBootLook()).toMatchObject({ theme: "light" });
  });
});
