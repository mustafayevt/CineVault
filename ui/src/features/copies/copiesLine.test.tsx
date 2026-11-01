import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { validateLibrarySearch } from "../../router";
import { LibraryPage } from "../library/LibraryPage";
import { setLibraryBackend } from "../library/libraryBackend";
import { createSyntheticBackend } from "../library/syntheticLibrary";
import { CopiesLine } from "./CopiesLine";
import { setCopiesBackend } from "./copiesBackend";
import { createSyntheticCopies, resetSyntheticCopies } from "./syntheticCopies";

function renderRoutes(path: string, children?: React.ReactNode) {
  const root = createRootRoute();
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/library",
        validateSearch: validateLibrarySearch,
        component: LibraryPage,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/movies/$movieId",
        component: () => <>{children ?? <p>Movie page</p>}</>,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/movies/$movieId/copies",
        component: () => <p>Compare page</p>,
      }),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { router, client };
}

beforeEach(() => {
  resetSyntheticCopies();
  setLibraryBackend(createSyntheticBackend(181));
  setCopiesBackend(createSyntheticCopies());
});
afterEach(() => {
  setLibraryBackend(null);
  setCopiesBackend(null);
});

describe("the Movie page line", () => {
  it("reads You have 2 copies · Compare for an open group and a kept one", async () => {
    renderRoutes("/movies/5", <CopiesLine movieId="5" />);
    expect(await screen.findByText("You have 2 copies")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Compare" })).toHaveAttribute(
      "href",
      "/movies/5/copies",
    );
  });

  it("is still there after Keep both", async () => {
    await createSyntheticCopies().keepAll(["5", "6"]);
    renderRoutes("/movies/5", <CopiesLine movieId="5" />);
    expect(await screen.findByText("You have 2 copies")).toBeInTheDocument();
  });

  it("is absent for a movie without copies", async () => {
    renderRoutes("/movies/1", <CopiesLine movieId="1" />);
    await screen.findByText("Movie page").catch(() => undefined);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByTestId("copies-line")).toBeNull();
    expect(screen.queryByText(/You have/)).toBeNull();
  });
});
