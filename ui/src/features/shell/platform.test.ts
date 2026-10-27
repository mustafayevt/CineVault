import { describe, expect, it } from "vitest";

import { platformFromUserAgent } from "./platform";

describe("platformFromUserAgent", () => {
  it("classifies user agents", () => {
    expect(platformFromUserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) WebView2")).toBe(
      "windows",
    );
    expect(platformFromUserAgent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")).toBe("macos");
    expect(platformFromUserAgent("Mozilla/5.0 (X11; Linux x86_64)")).toBe("linux");
    expect(
      platformFromUserAgent(
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0",
      ),
    ).toBe("linux");
  });

  it("counts Android as other even though its agent says Linux", () => {
    expect(
      platformFromUserAgent("Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36"),
    ).toBe("other");
  });

  it("does not match a lower-case linux in a jsdom-style agent", () => {
    expect(platformFromUserAgent("Mozilla/5.0 (linux) AppleWebKit/537.36 jsdom/26")).toBe("other");
  });
});
