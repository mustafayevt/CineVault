import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useSettingsStore } from "../settings/settingsStore";
import { useTitleActions } from "./titleActions";

function Header() {
  const actions = useTitleActions({
    id: "1",
    kind: "movie",
    title: "Example Movie",
    folder: "D:/Movies/Example Movie (2020)",
    availability: "",
    playId: "1",
    onShowNfo: () => undefined,
  });
  return (
    <ul>
      {actions.header.map((action) => (
        <li key={action.id}>{action.id}</li>
      ))}
    </ul>
  );
}

async function headerIds(): Promise<string[]> {
  const root = createRootRoute({ component: Header });
  const router = createRouter({
    routeTree: root,
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  render(<RouterProvider router={router} />);
  const items = await screen.findAllByRole("listitem");
  return items.map((item) => item.textContent ?? "");
}

afterEach(() => {
  useSettingsStore.setState({ values: {}, status: "ready" });
});

describe("the title page header", () => {
  it("offers Rename and Move", async () => {
    expect(await headerIds()).toEqual([
      "play",
      "scrape",
      "rename",
      "move",
      "open-folder",
      "artwork",
    ]);
  });

  it("leaves Rename and Move out while Metadata only is on", async () => {
    useSettingsStore.setState({ values: { "renamer.metadata-only": true }, status: "ready" });
    expect(await headerIds()).toEqual(["play", "scrape", "open-folder", "artwork"]);
  });
});
