import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StatusTag } from "./StatusTag";
import { STATUSES } from "./status";
import type { Status } from "./status";
import { Tag } from "./Tag";

const WORDS: Record<Status, string> = {
  done: "Done",
  waiting: "Waiting",
  missing: "Missing",
  "needs-key": "Needs a key",
  "in-progress": "In progress",
  off: "Off",
};

describe("StatusTag", () => {
  it.each(STATUSES)("renders a glyph and a word for %s", (status) => {
    const { container } = render(<StatusTag status={status} />);
    const tag = container.querySelector(`[data-status="${status}"]`);
    expect(tag).not.toBeNull();
    expect(tag?.querySelector("svg")).not.toBeNull();
    expect(tag?.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByText(WORDS[status])).toBeInTheDocument();
  });

  it("uses a more specific word when given one, keeping the glyph", () => {
    const { container } = render(<StatusTag status="missing">Trailer</StatusTag>);
    expect(screen.getByText("Trailer")).toBeInTheDocument();
    expect(container.querySelector("svg")).not.toBeNull();
  });

  it("makes off look different from missing, in glyph and in colour", () => {
    const { container } = render(
      <>
        <StatusTag status="off">Trailer</StatusTag>
        <StatusTag status="missing">Trailer</StatusTag>
      </>,
    );
    const off = container.querySelector<HTMLElement>('[data-status="off"]');
    const missing = container.querySelector<HTMLElement>('[data-status="missing"]');
    expect(off?.querySelector("svg")?.getAttribute("class")).not.toBe(
      missing?.querySelector("svg")?.getAttribute("class"),
    );
    expect(off?.className).toContain("text-text-muted");
    expect(off?.className).not.toContain("bg-");
    expect(missing?.className).toContain("text-danger");
    expect(missing?.className).toContain("bg-surface-active");
  });

  it("draws an expected gap amber, not red, with the same glyph and word", () => {
    const { container } = render(<StatusTag status="missing" tone="soft" />);
    const gap = container.querySelector<HTMLElement>('[data-status="missing"]');
    expect(gap).toHaveTextContent("Missing");
    expect(gap?.className).toContain("bg-warn-tint");
    expect(gap?.className).not.toContain("danger");
  });

  it("gives every state its own glyph", () => {
    const { container } = render(
      <>
        {STATUSES.map((status) => (
          <StatusTag key={status} status={status} />
        ))}
      </>,
    );
    const glyphs = [...container.querySelectorAll("svg")].map((svg) => svg.getAttribute("class"));
    expect(new Set(glyphs).size).toBe(STATUSES.length);
  });
});

describe("Tag", () => {
  it("sets every variant in sans, with tabular figures for technical values", () => {
    const { container } = render(
      <>
        <Tag variant="neutral">Not watched</Tag>
        <Tag variant="ok">HEVC Main 10</Tag>
      </>,
    );
    const [plain, technical] = [...container.querySelectorAll("span")];
    // One 11/600 box with tabular figures for every variant.
    expect(plain?.className).toContain("text-(length:--fs-11)");
    expect(plain?.className).not.toContain("type-code");
    expect(technical?.className).toContain("tabular-nums");
  });

  it.each(["neutral", "ok", "warn"] as const)("renders the %s variant", (variant) => {
    const { container } = render(<Tag variant={variant}>Label</Tag>);
    expect(container.querySelector(`[data-variant="${variant}"]`)).toHaveTextContent("Label");
  });
});
