import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Copy, Pencil, Trash2, X } from "lucide-react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Button } from "./Button";
import { ContextMenu } from "./ContextMenu";
import { Dialog } from "./Dialog";
import { IconButton } from "./IconButton";
import { Menu, MenuLabel, MenuRadioGroup, MenuRadioItem, MenuSeparator } from "./Menu";
import { MenuItem } from "./MenuItem";
import { useLateFlag } from "./motionHooks";
import { Popover } from "./Popover";
import { Presence } from "./Presence";
import { Select } from "./Select";
import { Tooltip } from "./Tooltip";

afterEach(() => {
  delete document.documentElement.dataset.motion;
  vi.useRealTimers();
});

function Toggle({ initial = true, exitMs }: { initial?: boolean; exitMs?: number }) {
  const [present, setPresent] = useState(initial);
  return (
    <>
      <button type="button" onClick={() => setPresent((p) => !p)}>
        toggle
      </button>
      <Presence present={present} exitMs={exitMs}>
        {(props) => (
          <div {...props} className="cv-slide-up" data-testid="bar">
            bar
          </div>
        )}
      </Presence>
    </>
  );
}

describe("Presence", () => {
  it("keeps the element for its exit animation and removes it when the animation ends", () => {
    render(<Toggle />);
    expect(screen.getByTestId("bar")).toHaveAttribute("data-state", "open");
    fireEvent.click(screen.getByText("toggle"));
    const leaving = screen.getByTestId("bar");
    expect(leaving).toHaveAttribute("data-state", "closed");
    // jsdom has no AnimationEvent, so React listens for the prefixed name there.
    fireEvent(leaving, new Event("webkitAnimationEnd", { bubbles: true }));
    expect(screen.queryByTestId("bar")).toBeNull();
  });

  it("removes the element after exitMs when no animation runs, and mounts it again", () => {
    vi.useFakeTimers();
    render(<Toggle exitMs={180} />);
    fireEvent.click(screen.getByText("toggle"));
    act(() => vi.advanceTimersByTime(179));
    expect(screen.getByTestId("bar")).toHaveAttribute("data-state", "closed");
    act(() => vi.advanceTimersByTime(1));
    expect(screen.queryByTestId("bar")).toBeNull();
    fireEvent.click(screen.getByText("toggle"));
    expect(screen.getByTestId("bar")).toHaveAttribute("data-state", "open");
  });

  it("swaps the exit for an instant removal under reduced motion", () => {
    document.documentElement.dataset.motion = "reduced";
    render(<Toggle />);
    fireEvent.click(screen.getByText("toggle"));
    expect(screen.queryByTestId("bar")).toBeNull();
  });
});

describe("useLateFlag", () => {
  it("turns true only after the delay, and false at once when inactive", () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ active }) => useLateFlag(150, active), {
      initialProps: { active: true },
    });
    expect(result.current).toBe(false);
    act(() => vi.advanceTimersByTime(149));
    expect(result.current).toBe(false);
    act(() => vi.advanceTimersByTime(1));
    expect(result.current).toBe(true);
    rerender({ active: false });
    expect(result.current).toBe(false);
  });
});

describe("Popover", () => {
  it("opens from its trigger on the floating recipe, closes on Esc and returns focus", async () => {
    const user = userEvent.setup();
    render(
      <Popover aria-label="Filters" trigger={<Button>Filters</Button>}>
        <button type="button">Inside</button>
      </Popover>,
    );
    const trigger = screen.getByRole("button", { name: "Filters" });
    await user.click(trigger);
    const panel = screen.getByRole("dialog", { name: "Filters" });
    expect(panel.className).toContain("cv-pop");
    expect(panel.className).toContain("bg-surface-floating");
    expect(panel.className).toContain("rounded-(--r-10)");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("closes on a click outside", async () => {
    const user = userEvent.setup();
    render(
      <>
        <button type="button">Elsewhere</button>
        <Popover aria-label="Filters" trigger={<Button>Filters</Button>}>
          <p>Body</p>
        </Popover>
      </>,
    );
    await user.click(screen.getByRole("button", { name: "Filters" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Elsewhere" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("Menu", () => {
  it("shows 28px items with icons and key hints, at least 264 wide, and acts on selection", async () => {
    const user = userEvent.setup();
    const onRename = vi.fn();
    render(
      <Menu aria-label="Title actions" trigger={<Button>Actions</Button>}>
        <MenuLabel>Title</MenuLabel>
        <MenuItem icon={Pencil} shortcut="F2" onSelect={onRename}>
          Rename 4 titles
        </MenuItem>
        <MenuSeparator />
        <MenuItem icon={Trash2} shortcut="Del" danger>
          Delete to Recycle Bin
        </MenuItem>
      </Menu>,
    );
    const trigger = screen.getByRole("button", { name: "Actions" });
    await user.click(trigger);
    const menu = screen.getByRole("menu", { name: "Title actions" });
    expect(menu.className).toContain("min-w-[264px]");
    expect(menu.className).toContain("cv-pop");
    const rename = screen.getByRole("menuitem", { name: /Rename 4 titles/ });
    expect(rename.className).toContain("h-(--h-menu-item)");
    expect(rename.querySelector("kbd")).toHaveTextContent("F2");
    expect(screen.getByRole("menuitem", { name: /Delete/ }).className).toContain("text-danger");
    expect(screen.getByRole("separator")).toBeInTheDocument();
    await user.click(rename);
    expect(onRename).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("moves with the arrow keys and keeps a disabled-with-reason item from acting", async () => {
    const user = userEvent.setup();
    const onCopy = vi.fn();
    const onLocked = vi.fn();
    render(
      <Menu trigger={<Button>Actions</Button>}>
        <MenuItem icon={Copy} onSelect={onCopy}>
          Copy
        </MenuItem>
        <MenuItem disabledReason="Needs a match first" enabledBy="Scrape" onSelect={onLocked}>
          Rename
        </MenuItem>
      </Menu>,
    );
    screen.getByRole("button", { name: "Actions" }).focus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.keyboard("{ArrowDown}");
    await user.keyboard("{ArrowDown}");
    const locked = screen.getByRole("menuitem", { name: /Rename/ });
    expect(locked).toHaveFocus();
    expect(locked.className).toContain("text-text-disabled");
    await user.keyboard("{Enter}");
    expect(onLocked).not.toHaveBeenCalled();
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.keyboard("{ArrowUp}{Enter}");
    expect(onCopy).toHaveBeenCalledOnce();
  });

  it("marks the chosen item of a radio group", async () => {
    const user = userEvent.setup();
    const onValueChange = vi.fn();
    render(
      <Menu trigger={<Button>Sort</Button>}>
        <MenuRadioGroup value="title" onValueChange={onValueChange}>
          <MenuRadioItem value="title">Title</MenuRadioItem>
          <MenuRadioItem value="year">Year</MenuRadioItem>
        </MenuRadioGroup>
      </Menu>,
    );
    await user.click(screen.getByRole("button", { name: "Sort" }));
    expect(screen.getByRole("menuitemradio", { name: "Title" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await user.click(screen.getByRole("menuitemradio", { name: "Year" }));
    expect(onValueChange).toHaveBeenCalledWith("year");
  });
});

describe("ContextMenu", () => {
  function Rows({ onTarget }: { onTarget: (target: Element) => void }) {
    return (
      <ContextMenu
        aria-label="Title actions"
        onTarget={onTarget}
        menu={
          <>
            <MenuItem shortcut="Enter">Open</MenuItem>
            <MenuSeparator />
            <MenuItem shortcut="Del">Delete to Recycle Bin</MenuItem>
          </>
        }
      >
        <ul aria-label="Titles">
          <li>
            <button type="button" data-key="alien">
              Alien
            </button>
          </li>
          <li>
            <button type="button" data-key="heat">
              Heat
            </button>
          </li>
        </ul>
      </ContextMenu>
    );
  }

  it("opens on right-click after telling which item was pointed at", async () => {
    const user = userEvent.setup();
    const onTarget = vi.fn();
    render(<Rows onTarget={onTarget} />);
    await user.pointer({ keys: "[MouseRight]", target: screen.getByText("Heat") });
    expect(onTarget).toHaveBeenCalledWith(screen.getByText("Heat"));
    const menu = screen.getByRole("menu", { name: "Title actions" });
    expect(menu.className).toContain("min-w-[264px]");
    expect(screen.getByRole("menuitem", { name: /Open/ })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("opens from the keyboard with Shift+F10 and the menu key on the focused item", async () => {
    const user = userEvent.setup();
    const onTarget = vi.fn();
    render(<Rows onTarget={onTarget} />);
    screen.getByText("Alien").focus();
    await user.keyboard("{Shift>}{F10}{/Shift}");
    expect(onTarget).toHaveBeenLastCalledWith(screen.getByText("Alien"));
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    screen.getByText("Heat").focus();
    await user.keyboard("{ContextMenu}");
    expect(onTarget).toHaveBeenLastCalledWith(screen.getByText("Heat"));
    expect(screen.getByRole("menu")).toBeInTheDocument();
  });
});

describe("Tooltip", () => {
  it("shows after the pointer rests about 500 ms and hides on Esc", async () => {
    const user = userEvent.setup();
    render(
      <Tooltip content="The whole title">
        <span>Dr. Strangelove…</span>
      </Tooltip>,
    );
    await user.hover(screen.getByText("Dr. Strangelove…"));
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(await screen.findByRole("tooltip")).toHaveTextContent("The whole title");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("shows at once on keyboard focus and hides on scroll", async () => {
    const user = userEvent.setup();
    render(
      <Tooltip content="Scan folders now">
        <button type="button">Scan</button>
      </Tooltip>,
    );
    await user.tab();
    expect(screen.getByRole("tooltip")).toHaveTextContent("Scan folders now");
    const content = document.querySelector("[data-side]");
    expect(content?.className).toContain("cv-pop");
    fireEvent.scroll(document);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("is what an icon button shows for its name, with no native title", async () => {
    const user = userEvent.setup();
    render(<IconButton aria-label="Clear selection" icon={X} />);
    const button = screen.getByRole("button", { name: "Clear selection" });
    expect(button).not.toHaveAttribute("title");
    await user.tab();
    expect(screen.getByRole("tooltip")).toHaveTextContent("Clear selection");
  });
});

describe("Dialog", () => {
  it("keeps focus inside, closes on Esc and hands focus back to its trigger", async () => {
    const user = userEvent.setup();
    render(
      <Dialog
        title="Delete 4 titles?"
        description="They go to the Recycle Bin."
        trigger={<Button>Delete</Button>}
        footer={
          <>
            <Button size="lg">Cancel</Button>
            <Button size="lg" variant="danger">
              Delete
            </Button>
          </>
        }
      />,
    );
    const trigger = screen.getByRole("button", { name: "Delete" });
    await user.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Delete 4 titles?" });
    expect(dialog).toHaveAccessibleDescription("They go to the Recycle Bin.");
    expect(dialog.className).toContain("rounded-(--r-14)");
    expect(dialog.className).toContain("bg-surface-floating");
    for (let i = 0; i < 6; i += 1) {
      await user.tab();
      expect(dialog.contains(document.activeElement)).toBe(true);
    }
    // Six steps round three controls land on the close button, whose tooltip Esc closes first.
    expect(screen.getByRole("button", { name: "Close" })).toHaveFocus();
    expect(screen.getByRole("tooltip")).toHaveTextContent("Close");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("lets Esc close the innermost thing first: the menu, then the dialog", async () => {
    const user = userEvent.setup();
    render(
      <Dialog title="Move" defaultOpen>
        <Menu trigger={<Button>Destination</Button>}>
          <MenuItem>D:\Movies</MenuItem>
        </Menu>
      </Dialog>,
    );
    await user.click(screen.getByRole("button", { name: "Destination" }));
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(screen.getByRole("dialog", { name: "Move" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("Select on the Menu", () => {
  const options = [
    { value: "all", label: "All drives" },
    { value: "d", label: "D:" },
    { value: "e", label: "E:" },
  ];

  function Controlled({ onChange }: { onChange: (value: string) => void }) {
    const [value, setValue] = useState("all");
    return (
      <Select
        aria-label="Drive"
        options={options}
        value={value}
        onValueChange={(next) => {
          setValue(next);
          onChange(next);
        }}
      />
    );
  }

  it("opens our menu on click, checks the current value and picks another", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Controlled onChange={onChange} />);
    const select = screen.getByRole("combobox", { name: "Drive" });
    await user.click(select);
    expect(screen.getByRole("menu")).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: "All drives" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await user.click(screen.getByRole("menuitemradio", { name: "E:" }));
    expect(onChange).toHaveBeenCalledWith("e");
    expect(select).toHaveValue("e");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(select).toHaveFocus();
  });

  it("opens from the keyboard and picks with the arrows", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Controlled onChange={onChange} />);
    screen.getByRole("combobox", { name: "Drive" }).focus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("menu")).toBeInTheDocument();
    // The list opens on the chosen item, so one step down is the next drive.
    expect(screen.getByRole("menuitemradio", { name: "All drives" })).toHaveFocus();
    await user.keyboard("{ArrowDown}{Enter}");
    expect(onChange).toHaveBeenCalledWith("d");
  });

  it("still works as a native select for form tests and closes any open list", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Controlled onChange={onChange} />);
    await user.selectOptions(screen.getByRole("combobox", { name: "Drive" }), "D:");
    expect(onChange).toHaveBeenCalledWith("d");
    expect(screen.queryByRole("menu")).toBeNull();
  });
});
