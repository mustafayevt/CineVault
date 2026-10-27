import { describe, expect, it } from "vitest";

import { platformFromUserAgent } from "./platform";

describe("platformFromUserAgent", () => {
  it("classifies user agents", () => {
    expect(platformFromUserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) WebView2")).toBe(
      "windows",
    );
    expect(platformFromUserAgent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")).toBe("macos");
    expect(platformFromUserAgent("Mozilla/5.0 (X11; Linux x86_64)")).toBe("other");
  });
});
