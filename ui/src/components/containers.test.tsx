import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChevronDown, Film, FolderPen, HardDrive, Trash2 } from "lucide-react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Button } from "./Button";
import { Card, SectionLabel } from "./Card";
import { NavItem } from "./NavItem";
import { PosterTile } from "./PosterTile";
import { ProgressCard } from "./ProgressCard";
import { SelectionBar } from "./SelectionBar";
import { SKELETON_DELAY_MS, Skeleton } from "./Skeleton";
import { SpaceBar } from "./SpaceBar";
import { Toast, ToastStack } from "./Toast";

afterEach(() => {
  vi.useRealTimers();
});

describe("Card and SectionLabel", () => {
  it("is the card colour at radius 10 with a hairline edge and no shadow", () => {
    render(<Card aria-label="Facts">body</Card>);
    const card = screen.getByRole("region", { name: "Facts" });
    expect(card).toHaveClass("bg-surface-card", "rounded-(--r-10)", "border-line-section");
    expect(card.className).not.toContain("shadow");
    expect(card).toHaveClass("px-4", "py-3.5");
  });

  it("has a compact padding for narrow columns", () => {
    render(
      <Card compact aria-label="Small">
        x
      </Card>,
    );
    expect(screen.getByRole("region", { name: "Small" })).toHaveClass("px-3", "py-2.5");
  });

  it("labels a section in 11/600 muted capitals and keeps its action at the right", () => {
    render(<SectionLabel action="Edit">Cast</SectionLabel>);
    const heading = screen.getByRole("heading", { level: 2, name: "Cast" });
    expect(heading).toHaveClass("uppercase", "tracking-[0.06em]", "text-text-muted");
    expect(heading.className).toContain("text-(length:--fs-11)");
    expect(screen.getByText("Edit")).toBeInTheDocument();
  });
});

describe("NavItem", () => {
  it("is a 32px row with an icon, the label and a count", () => {
    render(<NavItem icon={Film} label="Library" count={12} />);
    const item = screen.getByRole("button", { name: /Library/ });
    expect(item).toHaveClass("h-(--h-nav)", "w-full", "text-text-2");
    expect(item).not.toHaveAttribute("aria-current");
    expect(within(item).getByText("12")).toHaveClass("bg-surface-active");
  });

  it("marks the active page with the active fill, a gold icon and aria-current", () => {
    render(<NavItem icon={Film} label="Library" active />);
    const item = screen.getByRole("button", { name: "Library" });
    expect(item).toHaveAttribute("aria-current", "page");
    expect(item).toHaveClass("bg-surface-active", "text-text", "font-(weight:--fw-600)");
    expect(item.querySelector("svg")).toHaveClass("text-gold");
  });

  it("colours a warning count and renders as a link when asked", () => {
    render(
      <NavItem
        as="a"
        href="#drives"
        icon={HardDrive}
        label="Drives"
        count="1 low"
        countTone="warn"
      />,
    );
    const link = screen.getByRole("link", { name: /Drives/ });
    expect(link).toHaveAttribute("href", "#drives");
    expect(within(link).getByText("1 low")).toHaveClass("bg-warn-tint", "text-warn");
  });

  it("collapses to an icon with the label and count as its name, and a tooltip on focus", async () => {
    const user = userEvent.setup();
    render(<NavItem icon={Film} label="Scrape" count={12} collapsed />);
    const item = screen.getByRole("button", { name: "Scrape, 12" });
    expect(item).toHaveClass("w-(--h-nav)");
    expect(screen.queryByText("Scrape")).toBeNull();
    expect(screen.getByTestId("nav-count-dot")).toBeInTheDocument();
    await user.tab();
    expect(item).toHaveFocus();
    expect((await screen.findAllByText("Scrape · 12")).length).toBeGreaterThan(0);
  });

  it("runs its click handler", async () => {
    const onClick = vi.fn();
    render(<NavItem icon={Film} label="Library" onClick={onClick} />);
    await userEvent.click(screen.getByRole("button", { name: "Library" }));
    expect(onClick).toHaveBeenCalledOnce();
  });
});

describe("SpaceBar", () => {
  it("scales a neutral fill to the used share inside a 3px groove", () => {
    render(<SpaceBar used={0.83} label="D: Media, 83% used" />);
    const bar = screen.getByRole("meter", { name: "D: Media, 83% used" });
    expect(bar).toHaveClass("h-[3px]", "bg-track");
    expect(bar).toHaveAttribute("aria-valuenow", "83");
    const fill = screen.getByTestId("space-fill");
    expect(fill).toHaveClass("cv-bar", "bg-bar-neutral");
    expect(fill.style.getPropertyValue("--progress")).toBe("0.83");
  });

  it("uses the warning bar when low and an empty groove when offline", () => {
    const { rerender } = render(<SpaceBar used={0.96} label="C" state="low" />);
    expect(screen.getByTestId("space-fill")).toHaveClass("bg-warn-bar");
    rerender(<SpaceBar used={0.5} label="Z" state="offline" />);
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "0");
    expect(screen.getByTestId("space-fill").style.getPropertyValue("--progress")).toBe("0");
  });

  it("clamps the share", () => {
    render(<SpaceBar used={3} label="Full" />);
    expect(screen.getByTestId("space-fill").style.getPropertyValue("--progress")).toBe("1");
  });
});

describe("ProgressCard", () => {
  it("shows the bold line, the muted percentage and a thin bar", () => {
    render(<ProgressCard title="Scraping 3 of 25" percent={12} />);
    const card = screen.getByTestId("progress-card");
    expect(card).toHaveClass("bg-surface-well", "rounded-(--r-6)");
    expect(within(card).getByText("Scraping 3 of 25")).toHaveClass("font-(weight:--fw-600)");
    expect(within(card).getByText("12%")).toHaveClass("text-text-muted");
    expect(screen.getByRole("progressbar", { name: "Scraping 3 of 25" })).toHaveAttribute(
      "aria-valuenow",
      "12",
    );
  });

  it("becomes a ring with the percentage when collapsed", () => {
    render(<ProgressCard title="Scraping 3 of 25" percent={40} collapsed />);
    expect(screen.queryByTestId("progress-card")).toBeNull();
    expect(screen.getByRole("progressbar", { name: "Scraping 3 of 25" })).toHaveAttribute(
      "aria-valuenow",
      "40",
    );
    expect(screen.getByTestId("progress-ring")).toHaveAttribute("stroke-dashoffset", "60");
  });

  it("names the current item in a tooltip", async () => {
    const user = userEvent.setup();
    render(<ProgressCard title="Scraping" percent={5} detail="Heat (1995): writing artwork" />);
    await user.hover(screen.getByTestId("progress-card"));
    expect((await screen.findAllByText("Heat (1995): writing artwork")).length).toBeGreaterThan(0);
  });
});

describe("SelectionBar", () => {
  const actions = [
    { id: "rename", label: "Rename", icon: FolderPen, onSelect: vi.fn() },
    { id: "delete", label: "Delete", icon: Trash2, danger: true, onSelect: vi.fn() },
  ];

  it("is a floating toolbar with the count, the size and the actions", async () => {
    const onSelect = vi.fn();
    render(
      <SelectionBar
        count={4}
        size="186.2 GB"
        actions={[{ id: "rename", label: "Rename", onSelect }]}
        onClear={() => {}}
      />,
    );
    const bar = screen.getByRole("toolbar", { name: "Bulk actions" });
    expect(bar).toHaveClass(
      "cv-slide-up",
      "bg-surface-floating",
      "rounded-(--r-14)",
      "shadow-floating",
    );
    expect(bar).toHaveAttribute("data-state", "open");
    expect(within(bar).getByText("4 selected")).toBeInTheDocument();
    expect(within(bar).getByText("186.2 GB")).toBeInTheDocument();
    await userEvent.click(within(bar).getByRole("button", { name: "Rename" }));
    expect(onSelect).toHaveBeenCalledOnce();
  });

  it("paints a destructive action in the danger colour and clears with the X button", async () => {
    const onClear = vi.fn();
    render(<SelectionBar count={2} actions={actions} onClear={onClear} />);
    expect(screen.getByRole("button", { name: "Delete" })).toHaveClass("text-danger");
    await userEvent.click(screen.getByRole("button", { name: "Clear selection" }));
    expect(onClear).toHaveBeenCalledOnce();
  });

  it("opens the More menu with the rest of the actions", async () => {
    const onSelect = vi.fn();
    render(
      <SelectionBar
        count={2}
        actions={actions}
        more={[{ id: "nfo", label: "Rewrite NFO", shortcut: "F4", onSelect }]}
        onClear={() => {}}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "More" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: /Rewrite NFO/ }));
    expect(onSelect).toHaveBeenCalledOnce();
  });

  it("has no More button without extra actions", () => {
    render(<SelectionBar count={2} actions={actions} onClear={() => {}} />);
    expect(screen.queryByRole("button", { name: "More" })).toBeNull();
  });

  it("is absent with no selection and leaves after its exit when the selection empties", () => {
    vi.useFakeTimers();
    const { rerender } = render(<SelectionBar count={0} actions={actions} onClear={() => {}} />);
    expect(screen.queryByRole("toolbar")).toBeNull();
    rerender(<SelectionBar count={3} actions={actions} onClear={() => {}} />);
    expect(screen.getByRole("toolbar")).toHaveAttribute("data-state", "open");
    rerender(<SelectionBar count={0} actions={actions} onClear={() => {}} />);
    // Still there for its exit animation, still showing the last count.
    expect(screen.getByRole("toolbar")).toHaveAttribute("data-state", "closed");
    expect(screen.getByText("3 selected")).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(300));
    expect(screen.queryByRole("toolbar")).toBeNull();
  });
});

describe("Toast", () => {
  it("shows the glyph, Undo with its key hint and merges a count", () => {
    render(
      <Toast
        message="Renamed 12 folders"
        glyph={FolderPen}
        count={3}
        onUndo={() => {}}
        undoShortcut="Ctrl Z"
        duration={null}
      />,
    );
    const toast = screen.getByRole("status");
    expect(toast).toHaveClass("cv-slide-up", "bg-surface-floating", "rounded-(--r-10)");
    expect(toast.querySelector("svg")).not.toBeNull();
    expect(within(toast).getByText("×3")).toBeInTheDocument();
    const undo = within(toast).getByRole("button", { name: /Undo/ });
    expect(within(undo).getByText("Ctrl Z").tagName).toBe("KBD");
  });

  it("hides the key hint when the action has no binding", () => {
    render(<Toast message="Done" onUndo={() => {}} undoShortcut="" duration={null} />);
    expect(screen.getByRole("button", { name: "Undo" }).querySelector("kbd")).toBeNull();
  });

  it("marks a failure with a danger dot and offers its action", async () => {
    const onClick = vi.fn();
    render(
      <Toast
        message="Could not scrape Heat"
        tone="error"
        action={{ label: "View in Activity", onClick }}
        duration={null}
      />,
    );
    expect(screen.getByTestId("toast-error-dot")).toHaveClass("bg-danger");
    await userEvent.click(screen.getByRole("button", { name: "View in Activity" }));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("stays for its exit when closed, then leaves", () => {
    vi.useFakeTimers();
    const { rerender } = render(<Toast message="Done" duration={null} />);
    rerender(<Toast message="Done" duration={null} open={false} />);
    expect(screen.getByRole("status")).toHaveAttribute("data-state", "closed");
    act(() => vi.advanceTimersByTime(300));
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("does not run its timer while closed", () => {
    vi.useFakeTimers();
    const onDismiss = vi.fn();
    render(<Toast message="Done" onDismiss={onDismiss} open={false} />);
    act(() => vi.advanceTimersByTime(20_000));
    expect(onDismiss).not.toHaveBeenCalled();
  });
});

describe("ToastStack", () => {
  it("shows the newest three, lifted above the selection bar", () => {
    render(
      <ToastStack inset={56}>
        {["One", "Two", "Three", "Four"].map((message) => (
          <Toast key={message} message={message} duration={null} />
        ))}
      </ToastStack>,
    );
    const region = screen.getByRole("region", { name: "Toasts" });
    expect(
      within(region)
        .getAllByRole("status")
        .map((toast) => toast.textContent),
    ).toEqual([
      expect.stringContaining("Two"),
      expect.stringContaining("Three"),
      expect.stringContaining("Four"),
    ]);
    expect(region).toHaveClass("fixed", "right-4");
    expect(region.style.bottom).toBe("calc(var(--space-4) + 56px)");
  });

  it("can sit inside a container instead of the window", () => {
    render(
      <ToastStack position="absolute">
        <Toast message="One" duration={null} />
      </ToastStack>,
    );
    expect(screen.getByRole("region", { name: "Toasts" })).toHaveClass("absolute");
  });
});

describe("Skeleton delay", () => {
  it("reserves its box at once but shows only after 150 ms", () => {
    vi.useFakeTimers();
    const { container } = render(<Skeleton shape="poster" />);
    const skeleton = container.firstElementChild;
    expect(skeleton).toHaveClass("aspect-[2/3]", "opacity-0");
    expect(skeleton).toHaveAttribute("data-visible", "false");
    act(() => vi.advanceTimersByTime(SKELETON_DELAY_MS - 1));
    expect(skeleton).toHaveAttribute("data-visible", "false");
    act(() => vi.advanceTimersByTime(1));
    expect(skeleton).toHaveAttribute("data-visible", "true");
    expect(skeleton).not.toHaveClass("opacity-0");
  });

  it("shows at once when asked", () => {
    const { container } = render(<Skeleton shape="line" width="40%" immediate />);
    expect(container.firstElementChild).toHaveAttribute("data-visible", "true");
  });
});

describe("PosterTile", () => {
  it("is a 2:3 poster with the title, year, quality and the stage badge", () => {
    render(<PosterTile title="Arrival" year="2016" quality="2160p" stage="scraped" />);
    const tile = screen.getByRole("button", { name: /Arrival/ });
    expect(tile).toHaveAttribute("aria-pressed", "false");
    const frame = screen.getByTestId("poster-frame");
    expect(frame).toHaveClass("aspect-[2/3]", "rounded-(--r-10)", "cv-lift");
    expect(screen.getByTestId("placeholder-poster")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Scraped" })).toBeInTheDocument();
    expect(screen.getByText("2016")).toBeInTheDocument();
    expect(screen.getByText("2160p")).toBeInTheDocument();
  });

  it("shows a gold outline and a check when selected", () => {
    render(<PosterTile title="Heat" stage="renamed" selected />);
    expect(screen.getByRole("button", { name: /Heat/ })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("poster-frame")).toHaveClass("outline-2", "outline-gold");
    expect(screen.getByTestId("poster-check")).toBeInTheDocument();
  });

  it("flags a title that is not matched with the warning badge and note", () => {
    render(
      <PosterTile
        title="Godzilla"
        year="????"
        stage="attention"
        stageLabel="Not matched"
        note="Not matched"
      />,
    );
    expect(screen.getByRole("img", { name: "Not matched" })).toBeInTheDocument();
    expect(screen.getByText("Not matched", { selector: "span.text-warn" })).toBeInTheDocument();
  });

  it("uses the artwork it is given instead of the generated poster", () => {
    render(<PosterTile title="Heat" stage="new" poster={<img alt="Heat poster" src="x.jpg" />} />);
    expect(screen.queryByTestId("placeholder-poster")).toBeNull();
    expect(screen.getByAltText("Heat poster")).toBeInTheDocument();
  });

  it("selects with Enter and Space like any button", async () => {
    const onClick = vi.fn();
    render(<PosterTile title="Heat" stage="new" onClick={onClick} />);
    await userEvent.tab();
    await userEvent.keyboard("{Enter}");
    await userEvent.keyboard(" ");
    expect(onClick).toHaveBeenCalledTimes(2);
  });
});

describe("Button trailing icon", () => {
  it("draws the icon after the label, outside the label text", () => {
    render(<Button trailingIcon={ChevronDown}>More</Button>);
    const button = screen.getByRole("button", { name: "More" });
    expect(button.lastElementChild?.tagName.toLowerCase()).toBe("svg");
    expect(button.firstElementChild?.tagName.toLowerCase()).toBe("span");
  });
});
