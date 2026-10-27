import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { splitPath } from "../lib/paths";
import { PathText } from "./PathText";

const PARENT = "C:\\Users\\alex\\Videos\\qa\\media";

describe("PathText", () => {
  it("splits a path into its parent folders and its last name", () => {
    expect(splitPath(`${PARENT}\\Movies`)).toEqual({ head: `${PARENT}\\`, leaf: "Movies" });
    expect(splitPath("/Volumes/Media/TV/")).toEqual({ head: "/Volumes/Media/", leaf: "TV" });
    expect(splitPath("\\\\nas\\share")).toEqual({ head: "\\\\nas\\", leaf: "share" });
    expect(splitPath("D:\\")).toEqual({ head: "", leaf: "D:\\" });
  });

  it("keeps the last folder name whole and lets the parent folders give way", () => {
    const { container } = render(
      <>
        <PathText path={`${PARENT}\\Movies`} />
        <PathText path={`${PARENT}\\TV`} />
      </>,
    );
    const rows = Array.from(container.querySelectorAll(":scope > span"));
    const leaves = rows.map((row) => row.children[1]);
    // The names that tell the rows apart are never what is cut.
    expect(leaves.map((leaf) => leaf?.textContent)).toEqual(["Movies", "TV"]);
    for (const row of rows) {
      expect(row.children[0]).toHaveClass("truncate", "min-w-0");
      expect(row.children[1]).toHaveClass("shrink-0");
    }
    // The full path is the tooltip and what a screen reader hears, once.
    expect(rows[0]).toHaveAttribute("title", `${PARENT}\\Movies`);
    expect(screen.getByText(`${PARENT}\\TV`)).toHaveClass("sr-only");
  });
});
