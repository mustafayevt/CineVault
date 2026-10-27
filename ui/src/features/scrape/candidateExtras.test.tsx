import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { CandidateExtras } from "./CandidateExtras";
import type { ScrapeFieldDiff } from "./scrapeBackend";

const row = (field: string, group: string, incoming: string | null): ScrapeFieldDiff => ({
  field,
  group,
  current: null,
  incoming,
  change: "added",
  locked: false,
  incomingImageKey: null,
});

describe("CandidateExtras", () => {
  it("says it is waiting while the details are on their way", () => {
    render(<CandidateExtras rows={null} />);
    expect(screen.getByTestId("candidate-extras-wait")).toBeTruthy();
  });

  it("shows the tagline, the facts the provider has and the first cast names", () => {
    const cast = Array.from({ length: 10 }, (_, index) => `Actor ${index + 1}`).join(", ");
    render(
      <CandidateExtras
        rows={[
          row("tagline", "text", "The ocean remembers."),
          row("genres", "text", "Drama, Science Fiction"),
          row("studio", "text", ""),
          row("actors", "people", cast),
        ]}
      />,
    );
    expect(screen.getByText("The ocean remembers.")).toBeTruthy();
    expect(screen.getByText("Drama, Science Fiction")).toBeTruthy();
    expect(screen.queryByText("Studios")).toBeNull();
    expect(screen.getByText(/Actor 8 and 2 more/)).toBeTruthy();
  });
});
