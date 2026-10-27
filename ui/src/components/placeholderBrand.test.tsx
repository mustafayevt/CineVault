import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { PlaceholderBackdrop, PlaceholderPoster } from "./PlaceholderPoster";
import { posterArt } from "./posterArt";

describe("generated art carries the brand", () => {
  it("signs a generated poster with the maker's mark in the caption ink", () => {
    render(<PlaceholderPoster title="Heat (1995)" />);
    const mark = screen.getByTestId("poster-mark");
    expect(mark).toHaveAttribute("fill", posterArt("Heat (1995)").ink);
    expect(mark.querySelectorAll("path")).toHaveLength(2);
  });

  it("builds a flat backdrop from the poster's own motif, with no gradient", () => {
    const { container } = render(<PlaceholderBackdrop title="Heat (1995)" />);
    expect(container.querySelector("[data-motif]")).toHaveAttribute(
      "data-motif",
      posterArt("Heat (1995)").motif,
    );
    expect(container.querySelector("linearGradient, radialGradient")).toBeNull();
  });
});
