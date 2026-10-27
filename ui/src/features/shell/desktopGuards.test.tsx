import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useSettingsStore } from "../settings/settingsStore";
import { ContextMenu } from "../../components/ContextMenu";
import { MenuItem } from "../../components/MenuItem";
import {
  DRAG_SOURCE_ATTRIBUTE,
  installDesktopGuards,
  LIBRARY_SEARCH_ATTRIBUTE,
} from "./desktopGuards";

let remove: () => void = () => {};
const focusSearch = vi.fn();
const back = vi.fn();
const forward = vi.fn();
const zoom = vi.fn();

beforeEach(() => {
  focusSearch.mockClear();
  back.mockClear();
  forward.mockClear();
  zoom.mockClear();
  remove = installDesktopGuards({
    focusSearch,
    back,
    forward,
    zoom,
    blockDevTools: true,
    mac: false,
  });
});

afterEach(() => {
  remove();
});

/** Fires a key and reports whether the guards cancelled it. */
function press(init: KeyboardEventInit & { key: string }): boolean {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  document.body.dispatchEvent(event);
  return event.defaultPrevented;
}

describe("blocked browser keys", () => {
  it.each([
    ["F5", { key: "F5" }],
    ["Ctrl+R", { key: "r", ctrlKey: true }],
    ["Ctrl+Shift+R", { key: "R", ctrlKey: true, shiftKey: true }],
    ["Cmd+R", { key: "r", metaKey: true }],
    ["Ctrl+P", { key: "p", ctrlKey: true }],
    ["Ctrl+U", { key: "u", ctrlKey: true }],
    ["Ctrl+G", { key: "g", ctrlKey: true }],
    ["Ctrl+Shift+G", { key: "G", ctrlKey: true, shiftKey: true }],
    ["F3", { key: "F3" }],
    ["Ctrl+plus", { key: "+", ctrlKey: true, shiftKey: true }],
    ["Ctrl+equals", { key: "=", ctrlKey: true }],
    ["Ctrl+minus", { key: "-", ctrlKey: true }],
    ["Ctrl+0", { key: "0", ctrlKey: true }],
    ["Cmd+plus", { key: "=", metaKey: true }],
    ["the browser refresh key", { key: "BrowserRefresh" }],
    ["the browser search key", { key: "BrowserSearch" }],
    ["F12", { key: "F12" }],
    ["Ctrl+Shift+I", { key: "I", ctrlKey: true, shiftKey: true }],
  ])("%s does nothing of its own", (_name, init) => {
    expect(press(init)).toBe(true);
    expect(focusSearch).not.toHaveBeenCalled();
  });

  it("leaves ordinary keys and the app's own chords alone", () => {
    expect(press({ key: "a" })).toBe(false);
    expect(press({ key: "Enter" })).toBe(false);
    expect(press({ key: "z", ctrlKey: true })).toBe(false);
    expect(press({ key: "a", ctrlKey: true })).toBe(false);
    expect(press({ key: "ArrowLeft", altKey: true })).toBe(false);
  });

  it("does not block the developer-tool keys when they are allowed", () => {
    remove();
    remove = installDesktopGuards({ blockDevTools: false });
    expect(press({ key: "F12" })).toBe(false);
    expect(press({ key: "I", ctrlKey: true, shiftKey: true })).toBe(false);
  });
});

describe("Ctrl+F", () => {
  it("focuses the library search and cancels the browser's find", () => {
    expect(press({ key: "f", ctrlKey: true })).toBe(true);
    expect(focusSearch).toHaveBeenCalledTimes(1);
  });

  it("works with the command key", () => {
    expect(press({ key: "f", metaKey: true })).toBe(true);
    expect(focusSearch).toHaveBeenCalledTimes(1);
  });

  it("by default focuses the marked search field and selects its text", () => {
    remove();
    remove = installDesktopGuards();
    render(<input aria-label="Find" defaultValue="heat" {...{ [LIBRARY_SEARCH_ATTRIBUTE]: "" }} />);
    press({ key: "f", ctrlKey: true });
    const field = screen.getByLabelText<HTMLInputElement>("Find");
    expect(field).toHaveFocus();
    expect(field.selectionStart).toBe(0);
    expect(field.selectionEnd).toBe(4);
  });
});

describe("the browser back and forward keys", () => {
  it("are cancelled and drive CineVault's own history", () => {
    expect(press({ key: "BrowserBack" })).toBe(true);
    expect(back).toHaveBeenCalledTimes(1);
    expect(press({ key: "BrowserForward" })).toBe(true);
    expect(forward).toHaveBeenCalledTimes(1);
  });
});

describe("the app zoom keys", () => {
  it.each([
    ["Ctrl+equals", { key: "=", ctrlKey: true }, "in"],
    ["Ctrl+plus", { key: "+", ctrlKey: true, shiftKey: true }, "in"],
    ["Ctrl+keypad plus", { key: "+", code: "NumpadAdd", ctrlKey: true }, "in"],
    ["Ctrl+minus", { key: "-", ctrlKey: true }, "out"],
    ["Ctrl+keypad minus", { key: "-", code: "NumpadSubtract", ctrlKey: true }, "out"],
    ["Ctrl+0", { key: "0", ctrlKey: true }, "reset"],
  ])("%s zooms the app and cancels the browser's zoom", (_name, init, action) => {
    expect(press(init)).toBe(true);
    expect(zoom).toHaveBeenCalledTimes(1);
    expect(zoom).toHaveBeenCalledWith(action);
  });

  it("uses Command on macOS, and Ctrl does nothing there", () => {
    remove();
    remove = installDesktopGuards({ zoom, mac: true });
    expect(press({ key: "=", metaKey: true })).toBe(true);
    expect(zoom).toHaveBeenCalledWith("in");
    zoom.mockClear();
    expect(press({ key: "=", ctrlKey: true })).toBe(true);
    expect(zoom).not.toHaveBeenCalled();
  });

  it("drives the zoom setting when no handler is given", () => {
    remove();
    remove = installDesktopGuards({ mac: false });
    useSettingsStore.setState({ values: {}, status: "ready" });
    press({ key: "=", ctrlKey: true });
    expect(useSettingsStore.getState().values["appearance.zoom"]).toBe(110);
    press({ key: "0", ctrlKey: true });
    expect(useSettingsStore.getState().values["appearance.zoom"]).toBeUndefined();
  });
});

describe("zoom by wheel", () => {
  it("cancels Ctrl+wheel and leaves a plain wheel alone", () => {
    const zoom = new WheelEvent("wheel", { bubbles: true, cancelable: true, ctrlKey: true });
    document.body.dispatchEvent(zoom);
    expect(zoom.defaultPrevented).toBe(true);
    const scroll = new WheelEvent("wheel", { bubbles: true, cancelable: true });
    document.body.dispatchEvent(scroll);
    expect(scroll.defaultPrevented).toBe(false);
  });
});

describe("ghost drags", () => {
  function dragStart(target: Element): boolean {
    const event = new Event("dragstart", { bubbles: true, cancelable: true });
    target.dispatchEvent(event);
    return event.defaultPrevented;
  }

  it("stops an image from starting a drag unless it is a drag source", () => {
    render(
      <div>
        <img alt="Poster" src="data:," />
        <div {...{ [DRAG_SOURCE_ATTRIBUTE]: "" }}>
          <img alt="Artwork tile" src="data:," />
        </div>
        <p>text</p>
      </div>,
    );
    expect(dragStart(screen.getByAltText("Poster"))).toBe(true);
    expect(dragStart(screen.getByAltText("Artwork tile"))).toBe(false);
    expect(dragStart(screen.getByText("text"))).toBe(false);
  });
});

describe("the browser context menu", () => {
  function rightClick(target: Element): boolean {
    const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    target.dispatchEvent(event);
    return event.defaultPrevented;
  }

  it("never shows, on empty space or on a field", () => {
    render(<input aria-label="Name" />);
    expect(rightClick(document.body)).toBe(true);
    expect(rightClick(screen.getByLabelText("Name"))).toBe(true);
  });

  it("still lets our own ContextMenu open on its targets", async () => {
    const user = userEvent.setup();
    const onTarget = vi.fn();
    render(
      <ContextMenu
        aria-label="Title actions"
        onTarget={onTarget}
        menu={
          <>
            <MenuItem shortcut="Enter">Open</MenuItem>
            <MenuItem shortcut="Del">Delete to Recycle Bin</MenuItem>
          </>
        }
      >
        <ul aria-label="Titles">
          <li>
            <button type="button">Heat</button>
          </li>
        </ul>
      </ContextMenu>,
    );
    await user.pointer({ keys: "[MouseRight]", target: screen.getByText("Heat") });
    expect(onTarget).toHaveBeenCalledWith(screen.getByText("Heat"));
    expect(screen.getByRole("menu", { name: "Title actions" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /Open/ })).toBeInTheDocument();
    // The keyboard route opens it too.
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    screen.getByText("Heat").focus();
    await user.keyboard("{Shift>}{F10}{/Shift}");
    expect(screen.getByRole("menu")).toBeInTheDocument();
  });
});
