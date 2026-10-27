export type Platform = "windows" | "macos" | "other";

/** Classifies a user-agent string. Pure, so it can be tested with any string. */
export function platformFromUserAgent(userAgent: string): Platform {
  if (/Windows/i.test(userAgent)) {
    return "windows";
  }
  if (/Mac OS X|Macintosh/i.test(userAgent)) {
    return "macos";
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
