import { render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { UpdatesSection } from "../settings/UpdatesSection";
import { useSettingsStore } from "../settings/settingsStore";
import { parseNotes, spansOf } from "./notesFormat";
import { useUpdateStore } from "./updateStore";
import type { UpdateOffer } from "./updatesBackend";

const offer = (notes: string): UpdateOffer => ({
  version: "1.2.0",
  current: "1.0.0",
  published: "2026-11-01T10:00:00Z",
  notes,
  size_bytes: 12_000_000,
});

describe("reading release notes", () => {
  it("reads headings, paragraphs and bullet lists, with blank lines between blocks", () => {
    const blocks = parseNotes(
      [
        "## Highlights",
        "Faster scans for big libraries.",
        "Less waiting.",
        "",
        "- Faster **Scan folders**",
        "- A fix for renamed folders,",
        "  which no longer lose their poster",
        "* Third",
        "",
        "1. Numbered",
      ].join("\r\n"),
    );
    expect(blocks).toEqual([
      { kind: "heading", text: "Highlights" },
      { kind: "paragraph", text: "Faster scans for big libraries. Less waiting." },
      {
        kind: "list",
        items: [
          "Faster **Scan folders**",
          "A fix for renamed folders, which no longer lose their poster",
          "Third",
        ],
      },
      { kind: "list", items: ["Numbered"] },
    ]);
    expect(parseNotes("  \n\n")).toEqual([]);
  });

  it("marks bold text and leaves an unclosed marker as typed", () => {
    expect(spansOf("a **b** c")).toEqual([
      { text: "a ", bold: false },
      { text: "b", bold: true },
      { text: " c", bold: false },
    ]);
    expect(spansOf("a **b")).toEqual([
      { text: "a ", bold: false },
      { text: "**b", bold: false },
    ]);
  });
});

describe("the release notes in Settings, Updates", () => {
  beforeEach(() => {
    useSettingsStore.setState({ values: {}, status: "ready" });
  });
  afterEach(() => {
    useUpdateStore.setState({ offer: null, phase: "idle" });
  });

  it("shows What's new in the offered version as a list, scrollable when long", () => {
    useUpdateStore.setState({
      phase: "available",
      offer: offer(
        "## Scanning\n- Faster scans\n- A fix for **renamed** folders\n\nThanks for testing.",
      ),
    });
    render(<UpdatesSection />);
    const notes = screen.getByRole("region", { name: "What's new in 1.2.0" });
    expect(within(notes).getByRole("heading", { name: "What's new in 1.2.0" })).toBeVisible();
    expect(within(notes).getByRole("heading", { name: "Scanning" })).toBeVisible();
    expect(
      within(notes)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["Faster scans", "A fix for renamed folders"]);
    expect(within(notes).getByText("renamed").tagName).toBe("STRONG");
    expect(within(notes).getByText("Thanks for testing.")).toBeVisible();
    // The block scrolls on its own and takes the keyboard.
    const scroller = notes.querySelector<HTMLElement>(".overflow-y-auto");
    expect(scroller).not.toBeNull();
    expect(scroller?.className).toContain("max-h-56");
    expect(scroller).toHaveAttribute("tabindex", "0");
  });

  it("never turns the notes into markup", () => {
    useUpdateStore.setState({
      phase: "available",
      offer: offer("<img src=x onerror=alert(1)> [link](https://example.test)"),
    });
    render(<UpdatesSection />);
    const notes = screen.getByTestId("release-notes");
    expect(notes.querySelector("img, a")).toBeNull();
    expect(notes).toHaveTextContent("<img src=x onerror=alert(1)> [link](https://example.test)");
  });

  it("shows nothing when there is no offer or the offer has no notes", () => {
    const { rerender } = render(<UpdatesSection />);
    expect(screen.queryByTestId("release-notes")).toBeNull();
    useUpdateStore.setState({ phase: "available", offer: offer("   ") });
    rerender(<UpdatesSection />);
    expect(screen.queryByTestId("release-notes")).toBeNull();
  });
});

describe("links in bundled notes", () => {
  it("are read only when asked for, and only for https addresses", () => {
    const text =
      "See [the page](https://example.test/a) and [bad](javascript:alert(1)) and **bold**";
    expect(spansOf(text).some((span) => span.href !== undefined)).toBe(false);
    const spans = spansOf(text, { links: true });
    expect(spans.filter((span) => span.href !== undefined)).toEqual([
      { text: "the page", bold: false, href: "https://example.test/a" },
    ]);
    expect(spans.map((span) => span.text).join("")).toBe(
      "See the page and [bad](javascript:alert(1)) and bold",
    );
    expect(spans.at(-1)).toEqual({ text: "bold", bold: true });
  });
});
