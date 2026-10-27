import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ContextMenu } from "../../components/ContextMenu";
import { Dialog } from "../../components/Dialog";
import { MenuItem } from "../../components/MenuItem";
import { installDesktopGuards } from "./desktopGuards";
import { escapeDepth, handleEscape, pushEscape, useEscapeLayer } from "./escStack";

let remove: () => void = () => {};

beforeEach(() => {
  remove = installDesktopGuards();
});

afterEach(() => {
  remove();
});

const press = { defaultPrevented: false, target: null } as const;

describe("the Esc stack", () => {
  it("closes menu, dialog, picker and selection in that order, one per press", () => {
    const order: string[] = [];
    const offs: Record<string, () => void> = {};
    for (const layer of ["selection", "menu", "picker", "dialog"] as const) {
      offs[layer] = pushEscape(layer, () => void order.push(layer));
    }
    for (let i = 0; i < 4; i += 1) {
      expect(handleEscape(press)).toBe(true);
      // The layer that just closed leaves the stack, as a real one would.
      offs[order[order.length - 1] ?? ""]?.();
    }
    expect(order).toEqual(["menu", "dialog", "picker", "selection"]);
    expect(handleEscape(press)).toBe(false);
    expect(escapeDepth()).toBe(0);
  });

  it("closes the newest of two layers of the same kind first", () => {
    const order: string[] = [];
    const first = pushEscape("picker", () => void order.push("first"));
    const second = pushEscape("picker", () => void order.push("second"));
    handleEscape(press);
    second();
    handleEscape(press);
    first();
    expect(order).toEqual(["second", "first"]);
  });

  it("passes the key on when a layer says it was not open", () => {
    const next = vi.fn();
    const offs = [pushEscape("selection", next), pushEscape("picker", () => false)];
    expect(handleEscape(press)).toBe(true);
    expect(next).toHaveBeenCalledTimes(1);
    offs.forEach((off) => off());
  });

  it("leaves a key that something already handled alone", () => {
    const close = vi.fn();
    const off = pushEscape("picker", close);
    expect(handleEscape({ defaultPrevented: true, target: null })).toBe(false);
    expect(close).not.toHaveBeenCalled();
    off();
  });

  it("never discards typed text: a field that holds text keeps the key", () => {
    const close = vi.fn();
    const off = pushEscape("picker", close);
    const field = document.createElement("input");
    field.value = "heat";
    expect(handleEscape({ defaultPrevented: false, target: field })).toBe(false);
    field.value = "";
    expect(handleEscape({ defaultPrevented: false, target: field })).toBe(true);
    expect(close).toHaveBeenCalledTimes(1);
    off();
  });
});

function Screen() {
  const [picker, setPicker] = useState(true);
  const [selected, setSelected] = useState(true);
  const [dialog, setDialog] = useState(true);
  useEscapeLayer("picker", picker, () => setPicker(false));
  useEscapeLayer("selection", selected, () => setSelected(false));
  return (
    <div>
      {picker ? <p>Picker page</p> : <p>Movie page</p>}
      {selected ? <p>3 selected</p> : null}
      <Dialog open={dialog} onOpenChange={setDialog} title="Delete permanently">
        <p>Dialog body</p>
      </Dialog>
    </div>
  );
}

describe("Esc with real layers", () => {
  it("closes the dialog, then the picker, then the selection", async () => {
    const user = userEvent.setup();
    render(<Screen />);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    // The dialog took that press; the picker and the selection are still there.
    expect(screen.getByText("Picker page")).toBeInTheDocument();
    expect(screen.getByText("3 selected")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.getByText("Movie page")).toBeInTheDocument();
    expect(screen.getByText("3 selected")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByText("3 selected")).toBeNull();
  });

  it("closes an open menu before the picker behind it", async () => {
    const user = userEvent.setup();
    function Page() {
      const [picker, setPicker] = useState(true);
      useEscapeLayer("picker", picker, () => setPicker(false));
      return (
        <ContextMenu aria-label="Actions" menu={<MenuItem>Open</MenuItem>}>
          <p>{picker ? "Picker page" : "Movie page"}</p>
        </ContextMenu>
      );
    }
    render(<Page />);
    await user.pointer({ keys: "[MouseRight]", target: screen.getByText("Picker page") });
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(screen.getByText("Picker page")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.getByText("Movie page")).toBeInTheDocument();
  });
});
