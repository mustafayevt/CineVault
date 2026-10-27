import { describe, expect, it } from "vitest";

import { textFormatError } from "./textFormats";

describe("the url format", () => {
  it("accepts http and https addresses, with a port or a path", () => {
    for (const ok of [
      "https://torrentio.strem.fun",
      "http://127.0.0.1:8191",
      "http://localhost:9117/api/v2.0/indexers/all/results/torznab/api",
      "  https://yts.mx/  ",
    ]) {
      expect(textFormatError("url", ok), ok).toBeNull();
    }
  });

  it("leaves an empty text to the setting's own rules", () => {
    expect(textFormatError("url", "")).toBeNull();
    expect(textFormatError("url", "   ")).toBeNull();
  });

  it("refuses other schemes, credentials, queries and fragments", () => {
    for (const bad of [
      "ftp://a.b",
      "javascript:alert(1)",
      "a.b",
      "https://user:pw@a.b",
      "https://a.b/?apikey=1",
      "https://a.b/#x",
      "magnet:?xt=urn:btih:abc",
    ]) {
      expect(textFormatError("url", bad), bad).not.toBeNull();
    }
  });
});
