export type Platform = "windows" | "macos" | "linux" | "other";

/**
 * Classifies a user-agent string. Pure, so it can be tested with any string. Android reports
 * "Linux" too, so it is checked first and counts as `other`. The Linux match is case-sensitive:
 * jsdom's default agent has a lower-case "linux" in it, and tests on Linux must stay `other`.
 */
export function platformFromUserAgent(userAgent: string): Platform {
  if (/Windows/i.test(userAgent)) {
    return "windows";
  }
  if (/Mac OS X|Macintosh/i.test(userAgent)) {
    return "macos";
  }
  if (/Android/.test(userAgent)) {
    return "other";
  }
  if (/Linux|X11/.test(userAgent)) {
    return "linux";
  }
  return "other";
}

/** The platform the UI is running on, from the webview's user agent. */
export function detectPlatform(): Platform {
  if (typeof navigator === "undefined") {
    return "other";
  }
  return platformFromUserAgent(navigator.userAgent);
}
