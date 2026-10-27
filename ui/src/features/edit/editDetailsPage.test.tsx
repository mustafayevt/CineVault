import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { restoreLayout, stubLayout } from "../../test/layout";
import { MovieEditPage, ShowEditPage } from "./EditDetailsPage";
import { setEditBackend } from "./editBackend";
import type { EditBackend } from "./editBackend";
import { createSyntheticEditBackend } from "./syntheticEdit";

function spyBackend() {
  const inner = createSyntheticEditBackend();
  const save = vi.fn(inner.save);
  const backend: EditBackend = { ...inner, save };
  setEditBackend(backend);
  return save;
}

function renderEdit(path = "/movies/1/edit") {
  const root = createRootRoute();
  const stub = (name: string) => () => <p>{name}</p>;
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/movies/$movieId/edit",
        component: MovieEditPage,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/tv/$showId/edit",
        component: ShowEditPage,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/movies/$movieId",
        component: stub("Movie page"),
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/tv/$showId",
        component: stub("Show page"),
      }),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

beforeEach(() => stubLayout());
afterEach(() => {
  restoreLayout();
  setEditBackend(null);
});

async function typeTitle(user: ReturnType<typeof userEvent.setup>, extra: string) {
  const title = await screen.findByRole("textbox", { name: "Title" });
  await user.type(title, extra);
  return title;
}

describe("Edit details", () => {
  it("groups the fields in sections and offers no tag or trailer editor", async () => {
    spyBackend();
    renderEdit();
    await screen.findByRole("textbox", { name: "Title" });
    for (const name of [
      "Titles",
      "Plot",
      "Release and file",
      "Cast and crew",
      "Artwork addresses",
    ]) {
      expect(screen.getAllByText(name).length).toBeGreaterThan(0);
    }
    expect(screen.queryByText("Tags")).toBeNull();
    expect(screen.queryByText(/trailer/i)).toBeNull();
    // No bar until something changes.
    expect(screen.queryByTestId("edit-foot-bar")).toBeNull();
  });

  it("counts unsaved changes in the foot bar and saves only what changed", async () => {
    const save = spyBackend();
    const user = userEvent.setup();
    const router = renderEdit();
    await typeTitle(user, "!");
    const bar = await screen.findByTestId("edit-foot-bar");
    expect(within(bar).getByText("1 change")).toBeInTheDocument();
    await user.click(within(bar).getByRole("button", { name: /Save/ }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    const changes = save.mock.calls[0]?.[1] ?? [];
    expect(changes.map((change) => change.key)).toEqual(["title"]);
    await waitFor(() => expect(router.state.location.pathname).toBe("/movies/1"));
  });

  it("saves with Ctrl+S", async () => {
    const save = spyBackend();
    const user = userEvent.setup();
    renderEdit();
    await typeTitle(user, "?");
    await user.keyboard("{Control>}s{/Control}");
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
  });

  it("refuses a value that cannot be saved and says why", async () => {
    const save = spyBackend();
    const user = userEvent.setup();
    renderEdit();
    const year = await screen.findByRole("spinbutton", { name: "Year" });
    await user.clear(year);
    await user.type(year, "99");
    expect(await screen.findByText(/1800/)).toBeInTheDocument();
    await user.click(
      within(await screen.findByTestId("edit-foot-bar")).getByRole("button", { name: /Save/ }),
    );
    expect(save).not.toHaveBeenCalled();
  });

  it("guards Back while there are unsaved changes, and nothing is lost silently", async () => {
    const save = spyBackend();
    const user = userEvent.setup();
    const router = renderEdit();
    await typeTitle(user, "!");
    await user.click(screen.getByRole("button", { name: "Back to the title" }));
    // The page stays and the bar asks.
    expect(await screen.findByText(/Leave with 1 unsaved change/)).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/movies/1/edit");
    await user.click(screen.getByRole("button", { name: "Keep editing" }));
    expect(router.state.location.pathname).toBe("/movies/1/edit");
    expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("Dune: Part Two!");

    await user.click(screen.getByRole("button", { name: "Back to the title" }));
    await user.click(await screen.findByRole("button", { name: "Discard and leave" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/movies/1"));
    expect(save).not.toHaveBeenCalled();
  });

  it("leaves without asking when nothing changed", async () => {
    spyBackend();
    const user = userEvent.setup();
    const router = renderEdit();
    await screen.findByRole("textbox", { name: "Title" });
    await user.click(screen.getByRole("button", { name: "Back to the title" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/movies/1"));
  });

  it("edits a show with its seasons and status", async () => {
    spyBackend();
    renderEdit("/tv/1/edit");
    await screen.findByRole("textbox", { name: "Title" });
    expect(screen.getByText("Seasons")).toBeInTheDocument();
    expect(screen.getByText("Status")).toBeInTheDocument();
  });
});
