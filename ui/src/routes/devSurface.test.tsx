import { afterEach, describe, expect, it, vi } from "vitest";

// The component gallery is a development aid: only a dev build routes it.
describe("development-only routes", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.resetModules();
  });

  it("routes the component gallery in a dev build", async () => {
    vi.stubEnv("DEV", true);
    vi.resetModules();
    const { router } = await import("../router");
    expect(Object.keys(router.routesByPath)).toContain("/gallery");
  });

  it("leaves the component gallery out of a production build", async () => {
    vi.stubEnv("DEV", false);
    vi.resetModules();
    const { router } = await import("../router");
    const paths = Object.keys(router.routesByPath);
    expect(paths).not.toContain("/gallery");
    expect(paths).not.toContain("/gallery/selection");
    // The real screens are still there.
    expect(paths).toContain("/library");
  });
});
