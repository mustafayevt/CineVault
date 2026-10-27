import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { BrandMark } from "./BrandMark";

describe("BrandMark", () => {
  it("draws the symbol and the wordmark, hidden from assistive technology by default", () => {
    const { container } = render(<BrandMark size={18} />);
    expect(screen.getByTestId("brand-symbol")).toHaveAttribute("width", "18");
    expect(screen.getByTestId("brand-wordmark")).toBeInTheDocument();
    expect(container.firstElementChild).toHaveAttribute("aria-hidden", "true");
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("draws the symbol alone for the rail", () => {
    render(<BrandMark variant="symbol" size={18} />);
    expect(screen.getByTestId("brand-symbol")).toBeInTheDocument();
    expect(screen.queryByTestId("brand-wordmark")).not.toBeInTheDocument();
  });

  it("is named CineVault when it stands alone", () => {
    render(<BrandMark variant="symbol" size={48} label="CineVault" />);
    expect(screen.getByRole("img", { name: "CineVault" })).toBeInTheDocument();
  });

  it("colours the frame with the accent and the ring with the text colour", () => {
    render(<BrandMark variant="symbol" />);
    const [ring, frame] = Array.from(screen.getByTestId("brand-symbol").querySelectorAll("path"));
    expect(ring).toHaveAttribute("fill", "currentColor");
    expect(frame).toHaveClass("fill-gold");
  });
});
