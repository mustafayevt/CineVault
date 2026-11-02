import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ReleaseTable } from "./ReleaseTable";
import { release } from "./torrentHarness";

/** Lays the work pane (`main`) out at a width, as the browser would on a phone or a tablet. */
function paneWidth(width: number) {
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(function (
    this: HTMLElement,
  ) {
    return this.tagName === "MAIN" ? width : 0;
  });
}

function renderTable() {
  render(
    <main>
      <ReleaseTable
        releases={[release()]}
        sources={[{ id: "torrentio", name: "Torrentio" }]}
        owned={null}
        sort={{ column: "seeders", direction: "desc" }}
        onSort={() => undefined}
      />
    </main>,
  );
  return screen.getAllByRole("columnheader").map((header) => header.textContent ?? "");
}

afterEach(() => vi.restoreAllMocks());

describe("the release table on narrow panes", () => {
  it("keeps every column at desktop widths", () => {
    paneWidth(1100);
    const headers = renderTable().join(" ");
    for (const label of ["Quality", "Release name", "Size", "Seeders", "Source", "Age"]) {
      expect(headers).toContain(label);
    }
  });

  it("leaves out the source and the age on a tablet", () => {
    paneWidth(804);
    const headers = renderTable().join(" ");
    expect(headers).toContain("Quality");
    expect(headers).toContain("Seeders");
    expect(headers).not.toContain("Source");
    expect(headers).not.toContain("Age");
  });

  it("keeps the name, the size and Send on a phone, with the quality in the name", () => {
    paneWidth(374);
    const headers = renderTable().join(" ");
    expect(headers).toContain("Release name");
    expect(headers).toContain("Size");
    for (const label of ["Quality", "Seeders", "Source", "Age"]) {
      expect(headers).not.toContain(label);
    }
  });
});
