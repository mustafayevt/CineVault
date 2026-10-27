import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ProgressBar } from "./ProgressBar";

describe("ProgressBar", () => {
  it("exposes its value and scales the fill instead of animating its width", () => {
    render(<ProgressBar label="Scanning" percent={40} />);
    const bar = screen.getByRole("progressbar", { name: "Scanning" });
    expect(bar).toHaveAttribute("aria-valuenow", "40");
    const fill = screen.getByTestId("progress-fill");
    expect(fill.style.getPropertyValue("--progress")).toBe("0.4");
    expect(fill.style.width).toBe("");
    expect(fill).toHaveClass("cv-bar");
  });

  it("clamps out-of-range and non-numeric values", () => {
    const { rerender } = render(<ProgressBar label="Job" percent={140} />);
    expect(screen.getByTestId("progress-fill").style.getPropertyValue("--progress")).toBe("1");
    rerender(<ProgressBar label="Job" percent={Number.NaN} />);
    expect(screen.getByTestId("progress-fill").style.getPropertyValue("--progress")).toBe("0");
  });

  it("is gold for local work and neutral for network work", () => {
    const { rerender } = render(<ProgressBar label="Providers" percent={10} />);
    expect(screen.getByTestId("progress-fill")).toHaveClass("bg-gold");
    rerender(<ProgressBar label="Providers" percent={10} tone="info" />);
    expect(screen.getByTestId("progress-fill")).toHaveClass("bg-bar-neutral");
  });

  it("is a thin 3px groove by default and 6px at the medium size", () => {
    const { rerender } = render(<ProgressBar label="Job" percent={50} />);
    expect(screen.getByRole("progressbar")).toHaveClass("h-[3px]", "bg-track");
    rerender(<ProgressBar label="Job" percent={50} size="md" />);
    expect(screen.getByRole("progressbar")).toHaveClass("h-1.5");
  });

  it("has no value while indeterminate", () => {
    render(<ProgressBar label="Waiting" percent={0} indeterminate />);
    expect(screen.getByRole("progressbar")).not.toHaveAttribute("aria-valuenow");
    expect(screen.getByTestId("progress-fill")).toHaveClass("cv-bar-indeterminate");
  });
});
