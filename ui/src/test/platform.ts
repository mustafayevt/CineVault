import { vi } from "vitest";

import type { Platform } from "../features/shell/platform";

/**
 * A user agent that `detectPlatform` reads as each platform. jsdom's own agent has a lower-case
 * "linux" in it, which reads as `other`.
 */
const USER_AGENTS: Readonly<Record<Platform, string>> = {
  windows: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko)",
  macos: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)",
  linux: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko)",
  other: "Mozilla/5.0 (linux) AppleWebKit/537.36 (KHTML, like Gecko) jsdom/26",
};

let restoreAgent: (() => void) | undefined;

/**
 * Makes `detectPlatform` report `platform`, so a test states the platform whose bin name it
 * expects. Call it in `beforeEach` (or in the test) and `restorePlatform` in `afterEach`.
 */
export function stubPlatform(platform: Platform): void {
  restorePlatform();
  const spy = vi.spyOn(window.navigator, "userAgent", "get").mockReturnValue(USER_AGENTS[platform]);
  restoreAgent = () => spy.mockRestore();
}

export function restorePlatform(): void {
  restoreAgent?.();
  restoreAgent = undefined;
}
