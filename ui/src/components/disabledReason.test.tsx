import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DropdownMenu } from "radix-ui";
import { describe, expect, it, vi } from "vitest";

import { Button } from "./Button";
import { MenuItem } from "./MenuItem";
import { reasonLine } from "./useDisabledReason";

const REASON = "Needs a match first";
const ENABLED_BY = "Scrape…";
const LINE = "Needs a match first · Scrape…";

describe("reasonLine", () => {
  it("joins the reason and the enabling action", () => {
    expect(reasonLine(REASON, ENABLED_BY)).toBe(LINE);
    expect(reasonLine(REASON)).toBe(REASON);
  });
});

describe("Button with a disabled reason", () => {
  function renderButton(onClick = vi.fn()) {
    render(
      <>
        <Button>Before</Button>
        <Button disabledReason={REASON} enabledBy={ENABLED_BY} onClick={onClick}>
          Scrape again
        </Button>
        <Button>After</Button>
      </>,
    );
    return onClick;
  }

  it("shows the reason and the enabling action on hover", async () => {
    const user = userEvent.setup();
    renderButton();
    expect(screen.queryByRole("tooltip")).toBeNull();
    await user.hover(screen.getByRole("button", { name: "Scrape again" }));
    expect(screen.getByRole("tooltip")).toHaveTextContent(LINE);
    await user.unhover(screen.getByRole("button", { name: "Scrape again" }));
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("shows the same line on keyboard focus, describes the button with it and hides on Escape", async () => {
    const user = userEvent.setup();
    renderButton();
    await user.tab();
    await user.tab();
    const button = screen.getByRole("button", { name: "Scrape again" });
    expect(button).toHaveFocus();
    const tip = screen.getByRole("tooltip");
    expect(tip).toHaveTextContent(LINE);
    expect(button).toHaveAttribute("aria-describedby", tip.id);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("tooltip")).toBeNull();
    await user.tab();
    expect(screen.getByRole("button", { name: "After" })).toHaveFocus();
  });

  it("stays in the tab order: aria-disabled, never the disabled attribute", async () => {
    const user = userEvent.setup();
    renderButton();
    const button = screen.getByRole("button", { name: "Scrape again" });
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).not.toBeDisabled();
    expect(button).not.toHaveAttribute("tabindex", "-1");
    await user.tab();
    await user.tab();
    expect(button).toHaveFocus();
  });

  it("does nothing when activated by mouse, Enter or Space", async () => {
    const user = userEvent.setup();
    const onClick = renderButton();
    const button = screen.getByRole("button", { name: "Scrape again" });
    await user.click(button);
    button.focus();
    await user.keyboard("{Enter}");
    await user.keyboard(" ");
    expect(onClick).not.toHaveBeenCalled();
  });

  it("behaves as a normal button without a reason", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Scrape</Button>);
    await user.click(screen.getByRole("button", { name: "Scrape" }));
    expect(onClick).toHaveBeenCalledOnce();
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(screen.getByRole("button")).not.toHaveAttribute("aria-disabled");
  });
});

describe("MenuItem with a disabled reason", () => {
  function renderMenu(onSelect = vi.fn(), onOther = vi.fn()) {
    render(
      <DropdownMenu.Root defaultOpen>
        <DropdownMenu.Trigger>Actions</DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content>
            <MenuItem onSelect={onOther}>Scrape…</MenuItem>
            <MenuItem disabledReason={REASON} enabledBy={ENABLED_BY} onSelect={onSelect}>
              Scrape again
            </MenuItem>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>,
    );
    return { onSelect, onOther };
  }

  it("is reachable with the arrow keys and shows the line on keyboard focus", async () => {
    const user = userEvent.setup();
    renderMenu();
    expect(screen.queryByRole("tooltip")).toBeNull();
    const item = screen.getByRole("menuitem", { name: /Scrape again/ });
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).not.toHaveAttribute("data-disabled");
    await user.keyboard("{ArrowDown}");
    await user.keyboard("{ArrowDown}");
    expect(item).toHaveFocus();
    expect(screen.getByRole("tooltip")).toHaveTextContent(LINE);
  });

  it("ignores selection and keeps the menu open", async () => {
    const user = userEvent.setup();
    const { onSelect } = renderMenu();
    const item = screen.getByRole("menuitem", { name: /Scrape again/ });
    await user.click(item);
    item.focus();
    await user.keyboard("{Enter}");
    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.getByRole("menu")).toBeInTheDocument();
  });

  it("runs an item without a reason as usual", async () => {
    const user = userEvent.setup();
    const { onOther } = renderMenu();
    await user.click(screen.getByRole("menuitem", { name: "Scrape…" }));
    expect(onOther).toHaveBeenCalledOnce();
  });
});
