import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { describe, expect, it } from "vitest";

import { GalleryPage } from "./GalleryPage";

/**
 * jsdom has no layout engine or canvas, so axe cannot measure rendered colours here. Colour
 * contrast is covered by `src/styles/a11y.contrast.test.ts`, which checks every token pair the
 * components use in both themes. This test runs every other axe rule on the rendered gallery.
 */
async function runAxe(root: HTMLElement) {
  return axe.run(root, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "best-practice"] },
    rules: { "color-contrast": { enabled: false }, "page-has-heading-one": { enabled: false } },
  });
}

const THEMES = ["Dark theme", "Light theme"] as const;

function panel(name: (typeof THEMES)[number]): HTMLElement {
  return screen.getByRole("region", { name });
}

describe("component gallery", () => {
  it("has a dark and a light column", () => {
    const { container } = render(<GalleryPage />);
    expect(
      screen.getByRole("heading", { level: 1, name: "Component gallery" }),
    ).toBeInTheDocument();
    const themes = [...container.querySelectorAll("section[data-theme]")].map((el) =>
      el.getAttribute("data-theme"),
    );
    expect(themes).toEqual(["dark", "light"]);
  });

  it("shows every control in each state, in both columns", () => {
    render(<GalleryPage />);
    for (const name of THEMES) {
      const column = panel(name);
      // Buttons: every look at every size, and each forced state.
      for (const label of ["Primary", "Default", "Ghost", "Danger"]) {
        expect(
          within(column).getAllByRole("button", { name: label }).length,
        ).toBeGreaterThanOrEqual(8);
      }
      expect(
        within(column)
          .getAllByRole("button", { name: "Rename" })
          .some((button) => button.getAttribute("aria-disabled") === "true"),
      ).toBe(true);
      expect(within(column).getByRole("button", { name: "Pressed toggle" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
      // Fields: rest, error and disabled, plus the search field and the selects.
      expect(within(column).getByRole("searchbox", { name: "Search the library" })).toHaveValue(
        "heat",
      );
      expect(within(column).getByLabelText("Disabled")).toBeDisabled();
      expect(within(column).getByText(/Unknown token/)).toBeInTheDocument();
      expect(within(column).getAllByRole("combobox").length).toBeGreaterThanOrEqual(3);
      expect(within(column).getByRole("checkbox", { name: "Mixed" })).toBePartiallyChecked();
      expect(within(column).getAllByRole("switch").length).toBe(4);
      // Selectors and chips.
      expect(within(column).getByRole("group", { name: "Library kind" })).toBeInTheDocument();
      expect(within(column).getByRole("button", { name: /^Scraped/ })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
      // Tags and badges.
      expect(column.querySelectorAll("[data-quality]").length).toBeGreaterThanOrEqual(5);
      for (const stage of ["new", "scraped", "renamed", "attention"]) {
        expect(column.querySelector(`[data-stage="${stage}"]`)).not.toBeNull();
      }
    }
  });

  it("shows the containers, bars and lists in both columns", () => {
    render(<GalleryPage />);
    for (const name of THEMES) {
      const column = panel(name);
      // The sidebar specimen, open and collapsed, with the active page marked.
      expect(within(column).getAllByRole("button", { name: /^Library/ })[0]).toHaveAttribute(
        "aria-current",
        "page",
      );
      expect(within(column).getAllByRole("button", { name: "Scrape, 12" })[0]).toBeInTheDocument();
      expect(within(column).getByText("1 low")).toBeInTheDocument();
      // Progress bars, space bars in all three states, the progress card as card and ring.
      expect(within(column).getByRole("progressbar", { name: "Scanning" })).toHaveAttribute(
        "aria-valuenow",
        "40",
      );
      expect(within(column).getByRole("progressbar", { name: "Waiting" })).not.toHaveAttribute(
        "aria-valuenow",
      );
      for (const state of ["normal", "low", "offline"]) {
        expect(column.querySelector(`[role="meter"][data-state="${state}"]`)).not.toBeNull();
      }
      expect(within(column).getAllByTestId("progress-card").length).toBe(1);
      expect(within(column).getAllByTestId("progress-ring").length).toBe(1);
      // Posters: one selected, one needing attention; skeletons of every shape.
      expect(within(column).getAllByTestId("poster-check")).toHaveLength(1);
      expect(within(column).getByText("Not matched", { selector: "span.text-warn" })).toBeTruthy();
      for (const shape of ["poster", "row", "line", "tile"]) {
        expect(column.querySelector(`[data-shape="${shape}"]`)).not.toBeNull();
      }
      // The table with its selected rows and the selection bar over it.
      const table = within(column).getByRole("grid", { name: "Titles" });
      // Rows are virtualised and jsdom has no layout, so only the header is measured here.
      expect(within(table).getByRole("columnheader", { name: "Title" })).toBeInTheDocument();
      expect(
        within(
          within(column).getAllByRole("toolbar", { name: "Bulk actions" })[0] as HTMLElement,
        ).getByText("2 selected"),
      ).toBeInTheDocument();
      // The toast stack of the gallery holds three toasts.
      const stack = within(column).getByRole("region", {
        name: `Toast demo (${name === "Dark theme" ? "dark" : "light"})`,
      });
      expect(within(stack).getAllByRole("status")).toHaveLength(3);
    }
  });

  it("shows the floating surfaces and draws a live menu inside its own column", async () => {
    const user = userEvent.setup();
    render(<GalleryPage />);
    const light = panel("Light theme");
    expect(within(light).getByRole("button", { name: "Delete titles…" })).toBeInTheDocument();
    await user.click(within(light).getByRole("button", { name: "Actions" }));
    const menu = await screen.findByRole("menu", { name: "Title actions" });
    // The menu is drawn in the light column, so it is light whatever theme the window has.
    expect(light.contains(menu)).toBe(true);
    expect(panel("Dark theme").contains(menu)).toBe(false);
    expect(
      within(menu).getByRole("menuitem", { name: /Delete to Recycle Bin/ }),
    ).toBeInTheDocument();
  });

  it("opens the dialog inside the frame of its column", async () => {
    const user = userEvent.setup();
    render(<GalleryPage />);
    const dark = panel("Dark theme");
    await user.click(within(dark).getByRole("button", { name: "Delete titles…" }));
    const dialog = await screen.findByRole("dialog", { name: "Move 3 titles to the Recycle Bin?" });
    expect(dark.contains(dialog)).toBe(true);
    await user.keyboard("{Escape}");
    await act(async () => {});
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("replays the motion primitives with their buttons", async () => {
    const user = userEvent.setup();
    render(<GalleryPage />);
    const dark = panel("Dark theme");
    // Selection bar toggles (the bar of the motion demo has one action, Rename).
    const toggle = within(dark).getByRole("button", { name: "Toggle selection bar" });
    expect(within(dark).getAllByRole("toolbar", { name: "Bulk actions" })).toHaveLength(2);
    await user.click(toggle);
    // It stays for its exit, then leaves.
    await act(() => new Promise((done) => setTimeout(done, 300)));
    expect(within(dark).getAllByRole("toolbar", { name: "Bulk actions" })).toHaveLength(1);
    // Expand opens its section.
    const expand = within(dark).getByRole("button", { name: "Expand" });
    await user.click(expand);
    expect(expand).toHaveAttribute("aria-expanded", "true");
    // The progress bars take the new value.
    await user.click(within(dark).getByRole("button", { name: "Advance 25%" }));
    expect(within(dark).getByRole("progressbar", { name: "Replay progress" })).toHaveAttribute(
      "aria-valuenow",
      "55",
    );
    // A toast is added to the motion stack.
    await user.click(within(dark).getByRole("button", { name: "Add toast" }));
    expect(
      within(within(dark).getByRole("region", { name: "Motion toast demo (dark)" })).getAllByRole(
        "status",
      ),
    ).toHaveLength(1);
  });

  it("has no axe violations in either theme", async () => {
    const { container } = render(<GalleryPage />);
    const results = await runAxe(container);
    const summary = results.violations.map(
      (v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
    );
    expect(summary).toEqual([]);
  });

  it("stays free of violations after interacting with the controls", async () => {
    const user = userEvent.setup();
    const { container } = render(<GalleryPage />);
    const dark = panel("Dark theme");
    await user.click(within(dark).getByRole("button", { name: "Toggle selection bar" }));
    await user.click(within(dark).getByRole("button", { name: "Add toast" }));
    await user.click(within(dark).getByRole("button", { name: "Expand" }));
    await user.click(within(dark).getByRole("button", { name: "Actions" }));
    const results = await runAxe(container);
    expect(results.violations.map((v) => v.id)).toEqual([]);
  });
});
