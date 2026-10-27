import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory, createRouter } from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it } from "vitest";

import { router as appRouter } from "../router";

// The real route tree on a memory history, so the sidebar targets are the ones users click.
function open(path: string) {
  const router = createRouter({
    routeTree: appRouter.routeTree,
    history: createMemoryHistory({ initialEntries: [path] }),
    defaultNotFoundComponent: appRouter.options.defaultNotFoundComponent,
  });
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

async function violations(container: HTMLElement) {
  const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
  return results.violations.map((violation) => violation.id);
}

describe("routes without a page", () => {
  it("shows a designed page, not the router's bare text", async () => {
    const { container } = open("/nowhere/at/all");
    expect(
      await screen.findByRole("heading", { name: "That page does not exist" }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/^Not Found$/)).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Go to Library" })).toHaveAttribute("href", "/library");
    expect(await violations(container)).toEqual([]);
  });
});
