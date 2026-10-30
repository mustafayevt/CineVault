import { afterEach, describe, expect, it, vi } from "vitest";

import { ArtworkRequestError, currentArtworkBackend } from "./artworkBackend";

type Flags = Record<string, unknown>;

afterEach(() => {
  delete (window as unknown as Flags).__CINEVAULT_TRANSPORT__;
  delete (globalThis as unknown as Flags).isTauri;
  vi.restoreAllMocks();
});

/** The server's page: the transport flag and the shim's `isTauri`, as `lib/ipc.ts` sets them. */
function serverPage(): void {
  (window as unknown as Flags).__CINEVAULT_TRANSPORT__ = "server";
  (globalThis as unknown as Flags).isTauri = true;
}

describe("artwork upload in a browser", () => {
  it("uploads a chosen file instead of refusing it as desktop-only", async () => {
    serverPage();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(JSON.stringify({ message: "That file is not an image." }), { status: 400 }),
      );
    const file = new File([new Uint8Array([1, 2, 3])], "poster.png", { type: "image/png" });
    await expect(
      currentArtworkBackend().placeLocal("1", "poster", { kind: "file", file }),
    ).rejects.toThrow("That file is not an image.");
    expect(fetchMock).toHaveBeenCalledWith("/api/artwork/upload", expect.anything());
  });

  it("names the size limit before sending a file the server would refuse", async () => {
    serverPage();
    const fetchMock = vi.spyOn(globalThis, "fetch");
    const file = new File([new Uint8Array(1)], "huge.png", { type: "image/png" });
    Object.defineProperty(file, "size", { value: 33 * 1024 * 1024 });
    const placing = currentArtworkBackend().placeLocal("1", "poster", { kind: "file", file });
    await expect(placing).rejects.toBeInstanceOf(ArtworkRequestError);
    await expect(placing).rejects.toThrow("larger than 32 MiB");
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
