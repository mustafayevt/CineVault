import { clearMocks, mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { CachedImage } from "./CachedImage";
import { ImageDeliveryProbe } from "./ImageDeliveryProbe";
import { imageUrl } from "./imageUrl";

const KEY = "0123456789abcdef0123456789abcdef";

declare global {
  // Set by the Tauri runtime; `isTauri()` reads it.
  var isTauri: boolean | undefined;
}

afterEach(() => {
  clearMocks();
  delete globalThis.isTauri;
});

describe("imageUrl", () => {
  it("uses the Windows form http://cv.localhost/img... inside the desktop app", () => {
    mockConvertFileSrc("windows");
    expect(imageUrl(KEY, "grid")).toBe(`http://cv.localhost/img%2F${KEY}%2Fgrid`);
  });

  it("uses the cv:// form on macOS and Linux", () => {
    mockConvertFileSrc("macos");
    expect(imageUrl(KEY, "hero")).toBe(`cv://localhost/img%2F${KEY}%2Fhero`);
    expect(imageUrl(KEY, "full")).toBe(`cv://localhost/img%2F${KEY}%2Ffull`);
  });

  it("gives a candidate shown before its file is written a URL of its own", () => {
    mockConvertFileSrc("windows");
    // The query follows the encoded path, so the protocol still reads the key and the size.
    expect(imageUrl(KEY, "full", { preview: true })).toBe(
      `http://cv.localhost/img%2F${KEY}%2Ffull?preview`,
    );
    expect(imageUrl(KEY, "full", { preview: false })).toBe(
      `http://cv.localhost/img%2F${KEY}%2Ffull`,
    );
  });

  it("falls back to the cv:// form in a plain browser", () => {
    expect(imageUrl(KEY, "list")).toBe(`cv://localhost/img/${KEY}/list`);
  });

  it("refuses anything that is not a content key", () => {
    for (const bad of ["", "abc", `${KEY}/..`, KEY.toUpperCase(), "../../etc/passwd"]) {
      expect(() => imageUrl(bad, "grid")).toThrow();
    }
  });
});

describe("CachedImage", () => {
  it("renders a lazy, async-decoded img whose src is the protocol URL", () => {
    mockConvertFileSrc("macos");
    render(<CachedImage imageKey={KEY} size="grid" alt="Poster of Heat" />);
    const img = screen.getByRole("img", { name: "Poster of Heat" });
    expect(img).toHaveAttribute("src", `cv://localhost/img%2F${KEY}%2Fgrid`);
    expect(img).toHaveAttribute("loading", "lazy");
    expect(img).toHaveAttribute("decoding", "async");
  });
});

describe("image delivery probe", () => {
  let calls: { cmd: string; args: unknown; result: unknown }[];

  beforeEach(() => {
    calls = [];
    globalThis.isTauri = true;
    mockConvertFileSrc("windows");
    mockIPC((cmd, args) => {
      const result = cmd === "register_image_source" ? { key: KEY } : null;
      calls.push({ cmd, args, result });
      return result;
    });
  });

  it("loads thumbnails through cv://img and no image bytes cross invoke()", async () => {
    const user = userEvent.setup();
    render(<ImageDeliveryProbe />);

    await user.type(screen.getByLabelText("Image file path"), "C:\\Media\\Heat\\poster.jpg");
    await user.click(screen.getByRole("button", { name: "Load image" }));

    const grid = await screen.findByRole("img", { name: "Grid thumbnail" });
    const list = screen.getByRole("img", { name: "List thumbnail" });
    expect(grid).toHaveAttribute("src", `http://cv.localhost/img%2F${KEY}%2Fgrid`);
    expect(list).toHaveAttribute("src", `http://cv.localhost/img%2F${KEY}%2Flist`);

    // The only IPC traffic: the path goes in, a 32-character key comes out.
    expect(calls).toEqual([
      {
        cmd: "register_image_source",
        args: { path: "C:\\Media\\Heat\\poster.jpg" },
        result: { key: KEY },
      },
    ]);
    const traffic = JSON.stringify(calls);
    expect(traffic).not.toMatch(/data:image|base64|blob:/i);
    expect(traffic).not.toMatch(/[A-Za-z0-9+/=]{200,}/);

    // No image uses an inline data or blob URL.
    for (const img of document.querySelectorAll("img")) {
      expect(img.getAttribute("src")).toMatch(/^http:\/\/cv\.localhost\/img%2F[0-9a-f]{32}%2F/);
    }
  });

  it("shows the refusal message when the file cannot be read", async () => {
    mockIPC((cmd) => {
      // A command that returns Err rejects with the error value, not an Error object.
      if (cmd === "register_image_source") {
        throw "That file cannot be read.";
      }
      return null;
    });
    const user = userEvent.setup();
    render(<ImageDeliveryProbe />);
    await user.type(screen.getByLabelText("Image file path"), "C:\\missing.jpg");
    await user.click(screen.getByRole("button", { name: "Load image" }));

    await waitFor(() => expect(screen.getByText("That file cannot be read.")).toBeInTheDocument());
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });
});
