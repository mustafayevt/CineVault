import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

// The files a browser installs the app from (spec `web-server` "Installable from a browser"):
// `ui/public/` is copied into the build as is, and the server serves these without a session.
const ui = resolve(__dirname, "..");
const brand = resolve(ui, "..", "assets", "brand", "favicon");
const ICONS = [
  "apple-touch-icon.png",
  "icon-192.png",
  "icon-512.png",
  "favicon.ico",
  "favicon.svg",
];

interface Manifest {
  name: string;
  short_name: string;
  start_url: string;
  scope: string;
  display: string;
  background_color: string;
  theme_color: string;
  icons: { src: string; sizes: string; type: string; purpose: string }[];
}

describe("installing the browser version as an app", () => {
  it("serves the brand icons unchanged", () => {
    for (const icon of ICONS) {
      expect(readFileSync(resolve(ui, "public", icon)), icon).toEqual(
        readFileSync(resolve(brand, icon)),
      );
    }
  });

  it("has a manifest that opens full screen on the library with icons that exist", () => {
    const manifest = JSON.parse(
      readFileSync(resolve(ui, "public", "manifest.webmanifest"), "utf8"),
    ) as Manifest;
    expect(manifest).toMatchObject({
      name: "CineVault",
      short_name: "CineVault",
      start_url: "/",
      scope: "/",
      display: "standalone",
    });
    // The dark window ground, the first frame the app shows.
    const tokens = readFileSync(resolve(ui, "src", "styles", "tokens.css"), "utf8");
    const ground = /--surface-window:\s*(#[0-9a-f]{6});/i.exec(tokens)?.[1];
    expect(manifest.background_color).toBe(ground);
    expect(manifest.theme_color).toBe(ground);
    expect(manifest.icons.map((icon) => `${icon.sizes} ${icon.purpose}`)).toEqual([
      "192x192 any",
      "512x512 any",
      "512x512 maskable",
    ]);
    for (const icon of manifest.icons) {
      expect(existsSync(resolve(ui, "public", icon.src.slice(1))), icon.src).toBe(true);
    }
  });

  it("leaves the install tags out of the desktop app, where the server adds them", () => {
    const index = readFileSync(resolve(ui, "index.html"), "utf8");
    expect(index).not.toContain("manifest.webmanifest");
  });
});
