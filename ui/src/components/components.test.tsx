import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Search } from "lucide-react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Button } from "./Button";
import { AddFilterChip, FilterChip } from "./Chip";
import { IconButton } from "./IconButton";
import { Field } from "./Field";
import { Kbd } from "./Kbd";
import { QualityBadge } from "./QualityBadge";
import { ScoreBadge } from "./ScoreBadge";
import { SegmentedControl } from "./SegmentedControl";
import { StageBadge } from "./StageBadge";
import { Tag } from "./Tag";
import { TOAST_DURATION_MS, Toast } from "./Toast";

afterEach(() => {
  vi.useRealTimers();
});

describe("Button", () => {
  it("renders the label, the shortcut hint and handles clicks", async () => {
    const onClick = vi.fn();
    render(
      <Button variant="primary" shortcut="Enter" onClick={onClick}>
        Accept
      </Button>,
    );
    await userEvent.click(screen.getByRole("button", { name: /Accept/ }));
    expect(onClick).toHaveBeenCalledOnce();
    // The one primary is gold.
    expect(screen.getByRole("button").className).toContain("bg-gold");
    expect(screen.getByRole("button").className).toContain("text-on-gold");
  });

  it("has four looks: primary, default, ghost and danger", () => {
    render(
      <>
        <Button variant="primary">P</Button>
        <Button>D</Button>
        <Button variant="ghost">G</Button>
        <Button variant="danger">X</Button>
      </>,
    );
    expect(screen.getByRole("button", { name: "P" }).className).toContain("bg-gold");
    expect(screen.getByRole("button", { name: "D" }).className).toContain("bg-surface-raised");
    expect(screen.getByRole("button", { name: "D" }).className).toContain("border-line-edge");
    expect(screen.getByRole("button", { name: "G" }).className).toContain("bg-transparent");
    expect(screen.getByRole("button", { name: "X" }).className).toContain("text-danger");
  });

  it("uses the 24 / 28 / 32 px button heights from the tokens", () => {
    render(
      <>
        <Button size="sm">Small</Button>
        <Button>Regular</Button>
        <Button size="lg">Large</Button>
      </>,
    );
    expect(screen.getByRole("button", { name: "Small" }).className).toContain("h-(--h-btn-sm)");
    expect(screen.getByRole("button", { name: "Regular" }).className).toContain("h-(--h-btn)");
    expect(screen.getByRole("button", { name: "Large" }).className).toContain("h-(--h-btn-lg)");
  });

  it("does not act when disabled and shows the disabled colour, not a faded block", async () => {
    const onClick = vi.fn();
    render(
      <Button variant="primary" disabled onClick={onClick}>
        Save
      </Button>,
    );
    const button = screen.getByRole("button", { name: "Save" });
    expect(button).toBeDisabled();
    await userEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
    expect(button.className).toContain("text-text-disabled");
    expect(button.className).not.toContain("bg-gold");
    expect(button.className).not.toContain("opacity");
  });

  it("stays focusable but inert with a disabled reason", async () => {
    const onClick = vi.fn();
    render(
      <Button disabledReason="Needs a match first" onClick={onClick}>
        Rename
      </Button>,
    );
    const button = screen.getByRole("button", { name: "Rename" });
    expect(button).toHaveAttribute("aria-disabled", "true");
    await userEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });
});

describe("IconButton", () => {
  it("names itself without a second description and draws the icon at the 1.75 stroke", () => {
    render(<IconButton aria-label="Search library" icon={Search} />);
    const button = screen.getByRole("button", { name: "Search library" });
    expect(button).toHaveAccessibleDescription("");
    expect(button.className).toContain("size-(--h-btn)");
    expect(button.querySelector("svg")).toHaveAttribute("stroke-width", "1.75");
  });

  it("reports pressed state and can be disabled", async () => {
    const onClick = vi.fn();
    const { rerender } = render(
      <IconButton aria-label="Table view" icon={Search} pressed onClick={onClick} />,
    );
    expect(screen.getByRole("button")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button").className).toContain("bg-surface-active");
    rerender(<IconButton aria-label="Table view" icon={Search} disabled onClick={onClick} />);
    await userEvent.click(screen.getByRole("button"));
    expect(onClick).not.toHaveBeenCalled();
    expect(screen.getByRole("button").className).toContain("text-text-disabled");
  });

  it("comes in 24, 28 and 32 px", () => {
    render(
      <>
        <IconButton aria-label="a" icon={Search} size="sm" />
        <IconButton aria-label="b" icon={Search} size="lg" />
      </>,
    );
    expect(screen.getByRole("button", { name: "a" }).className).toContain("size-(--h-btn-sm)");
    expect(screen.getByRole("button", { name: "b" }).className).toContain("size-(--h-btn-lg)");
  });
});

describe("Kbd", () => {
  it("is a key cap with the key edge token", () => {
    render(<Kbd>Shift Del</Kbd>);
    const key = screen.getByText("Shift Del");
    expect(key.tagName).toBe("KBD");
    expect(key.className).toContain("border-line-kbd");
    expect(key.className).toContain("rounded-(--r-4)");
  });
});

describe("Tag", () => {
  it("is 18px with the radius 4 / 11 / 600 box in three looks", () => {
    render(
      <>
        <Tag>Drama</Tag>
        <Tag variant="ok">Scraped</Tag>
        <Tag variant="warn">Rename conflict</Tag>
      </>,
    );
    for (const name of ["Drama", "Scraped", "Rename conflict"]) {
      const cls = screen.getByText(name).className;
      expect(cls).toContain("h-(--h-tag)");
      expect(cls).toContain("rounded-(--r-4)");
      expect(cls).toContain("text-(length:--fs-11)");
    }
    expect(screen.getByText("Drama").className).toContain("bg-surface-active");
    expect(screen.getByText("Scraped").className).toContain("text-ok");
    expect(screen.getByText("Rename conflict").className).toContain("text-warn");
  });
});

describe("QualityBadge", () => {
  it("uses one style for every tier and adds the HDR word", () => {
    render(
      <>
        <QualityBadge quality="2160p" hdr />
        <QualityBadge quality="1080p" />
        <QualityBadge quality="720p" />
        <QualityBadge quality="SD" />
        <QualityBadge quality="2160p" hdr="Dolby Vision" />
      </>,
    );
    const first = screen.getByText("HDR").parentElement;
    expect(first).toHaveTextContent("2160pHDR");
    expect(screen.getByText("Dolby Vision")).toBeInTheDocument();
    const classes = ["1080p", "720p", "SD"].map((q) => screen.getByText(q).className);
    for (const cls of classes) {
      expect(cls).toBe(classes[0]);
      expect(cls).toContain("h-(--h-tag)");
    }
    // The tier alone never picks a colour.
    expect(first?.className).toContain("bg-surface-active");
  });
});

describe("ScoreBadge", () => {
  it("is a tag with 700 tabular figures and an optional source in its name", () => {
    render(
      <>
        <ScoreBadge value={8.1} source="IMDb" />
        <ScoreBadge value="93%" />
      </>,
    );
    const imdb = screen.getByLabelText("IMDb 8.1");
    expect(imdb).toHaveTextContent("8.1");
    expect(imdb.className).toContain("font-(weight:--fw-700)");
    expect(imdb.className).toContain("tabular-nums");
    expect(screen.getByText("93%").className).toContain("h-(--h-tag)");
  });
});

describe("StageBadge", () => {
  it("names every stage: a check, a warning and a hollow dot", () => {
    render(
      <>
        <StageBadge stage="new" />
        <StageBadge stage="scraped" />
        <StageBadge stage="renamed" />
        <StageBadge stage="attention" label="Not matched" />
      </>,
    );
    expect(screen.getByRole("img", { name: "New" }).querySelector("svg")).toBeNull();
    const scraped = screen.getByRole("img", { name: "Scraped" }).querySelector("svg");
    expect(scraped).not.toBeNull();
    expect(scraped?.getAttribute("class")).toContain("text-ok");
    expect(screen.getByRole("img", { name: "Renamed" }).querySelector("svg")).not.toBeNull();
    const warning = screen.getByRole("img", { name: "Not matched" }).querySelector("svg");
    expect(warning?.getAttribute("class")).toContain("text-warn");
  });
});

describe("SegmentedControl", () => {
  const options = [
    { value: "browse", label: "Browse" },
    { value: "audit", label: "Audit" },
  ] as const;

  it("marks the active segment and reports changes", async () => {
    const onChange = vi.fn();
    render(
      <SegmentedControl
        aria-label="Library lens"
        options={options}
        value="browse"
        onChange={onChange}
      />,
    );
    expect(screen.getByRole("group", { name: "Library lens" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Browse" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Audit" })).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(screen.getByRole("button", { name: "Audit" }));
    expect(onChange).toHaveBeenCalledWith("audit");
  });

  it("marks the chosen segment with a raised thumb, never the accent", () => {
    render(
      <>
        <SegmentedControl aria-label="Lens" options={options} value="browse" onChange={() => {}} />
        <SegmentedControl
          aria-label="View"
          options={[{ value: "grid", icon: Search, ariaLabel: "Grid view" }]}
          value="grid"
          onChange={() => {}}
        />
      </>,
    );
    const text = screen.getByRole("button", { name: "Browse" });
    expect(text.className).toContain("bg-surface-active");
    expect(text.className).not.toContain("bg-gold");
    const iconSegment = screen.getByRole("button", { name: "Grid view" });
    expect(iconSegment.className).toContain("bg-surface-active");
    expect(iconSegment.className).not.toContain("bg-gold");
  });
});

describe("FilterChip", () => {
  it("removes with an accessible name that includes field and value", async () => {
    const onRemove = vi.fn();
    render(<FilterChip field="Year" value="2010..2019" onRemove={onRemove} />);
    await userEvent.click(screen.getByRole("button", { name: "Remove filter Year 2010..2019" }));
    expect(onRemove).toHaveBeenCalledOnce();
  });

  it("shows a negated chip with the word not and a struck-through value", () => {
    render(<FilterChip field="Watched" value="yes" negated />);
    expect(screen.getByText("not")).toBeInTheDocument();
    expect(screen.getByText("yes").className).toContain("line-through");
  });

  it("renders the Filter verb as a quiet button, not a dashed pill", () => {
    render(<AddFilterChip />);
    const add = screen.getByRole("button", { name: "Filter" });
    expect(add.className).not.toContain("border-dashed");
    expect(add.className).not.toContain("rounded-(--r-pill)");
  });
});

describe("Field", () => {
  it("connects label, description and error to the field", () => {
    render(<Field label="Folder template" description="Use tokens." error="Unknown token." />);
    const input = screen.getByLabelText("Folder template");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAccessibleDescription("Unknown token.");
  });

  it("describes the field with its helper text when there is no error", () => {
    render(
      <Field label="Quick search" description="Typed filters turn into chips." shortcut="Ctrl K" />,
    );
    expect(screen.getByLabelText("Quick search")).toHaveAccessibleDescription(
      "Typed filters turn into chips.",
    );
    expect(screen.getByText("Ctrl K")).toBeInTheDocument();
  });

  it("accepts typing", async () => {
    render(<Field label="Title" />);
    await userEvent.type(screen.getByLabelText("Title"), "Arrival");
    expect(screen.getByLabelText("Title")).toHaveValue("Arrival");
  });
});

describe("Toast", () => {
  it("offers Undo and Dismiss", async () => {
    const onUndo = vi.fn();
    const onDismiss = vi.fn();
    render(
      <Toast message="Marked 12 watched" onUndo={onUndo} onDismiss={onDismiss} duration={null} />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Marked 12 watched");
    await userEvent.click(screen.getByRole("button", { name: /Undo/ }));
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(onUndo).toHaveBeenCalledOnce();
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("dismisses itself after six seconds", () => {
    vi.useFakeTimers();
    const onDismiss = vi.fn();
    render(<Toast message="Marked 12 watched" onDismiss={onDismiss} />);
    act(() => vi.advanceTimersByTime(TOAST_DURATION_MS - 1));
    expect(onDismiss).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(1));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("leads with the action's glyph and shows a time bar only while it times out", () => {
    const { rerender } = render(
      <Toast message="Renamed 12 folders" glyph={Search} onDismiss={vi.fn()} />,
    );
    expect(screen.getByRole("status").querySelector("svg")).not.toBeNull();
    const bar = screen.getByTestId("toast-time");
    expect(bar.style.animationDuration).toBe(`${TOAST_DURATION_MS}ms`);
    expect(bar.style.animationPlayState).toBe("running");
    rerender(<Toast message="Renamed 12 folders" onDismiss={vi.fn()} duration={null} />);
    expect(screen.queryByTestId("toast-time")).toBeNull();
  });

  it("pauses its time bar while hovered", () => {
    render(<Toast message="Renamed 3 folders" onDismiss={vi.fn()} />);
    act(() => {
      screen.getByRole("status").dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    });
    expect(screen.getByTestId("toast-time").style.animationPlayState).toBe("paused");
  });

  it("stays while hovered and restarts when the pointer leaves", () => {
    vi.useFakeTimers();
    const onDismiss = vi.fn();
    render(<Toast message="Renamed 3 folders" onDismiss={onDismiss} />);
    const toast = screen.getByRole("status");
    act(() => {
      toast.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    });
    act(() => vi.advanceTimersByTime(TOAST_DURATION_MS * 3));
    expect(onDismiss).not.toHaveBeenCalled();
    act(() => {
      toast.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    });
    act(() => vi.advanceTimersByTime(TOAST_DURATION_MS));
    expect(onDismiss).toHaveBeenCalledOnce();
  });
});
