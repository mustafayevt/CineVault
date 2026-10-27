import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { MovieTags } from "./MovieTags";

describe("MovieTags", () => {
  it("lists each tag as a chip under a Tags label", () => {
    render(<MovieTags tags={["desert", "sandworm", " "]} />);
    expect(screen.getByText("Tags")).toBeTruthy();
    expect(screen.getByTestId("movie-tags").querySelectorAll("li")).toHaveLength(2);
  });

  it("renders nothing for a title without tags", () => {
    const { container } = render(<MovieTags tags={[]} />);
    expect(container.innerHTML).toBe("");
  });
});
