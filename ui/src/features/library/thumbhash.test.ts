import { describe, expect, it } from "vitest";

import { decodeThumbHash, thumbHashAspectRatio, thumbHashFromHex } from "./thumbhash";

// Hashes produced by the Rust encoder (`cv_artwork`'s `thumbhash` crate) for 2:3 gradients, with the
// pixels its own decoder returns. The TypeScript decoder must agree with it.
const SAMPLES = [
  {
    hex: "d1690a2d0888887f7787778777788887887f87f887",
    first: [185, 87, 25, 255],
    mid: [102, 57, 22, 255],
  },
  {
    hex: "4db6091d0688887f77877788777888877780770778",
    first: [23, 93, 145, 255],
    mid: [13, 44, 76, 255],
  },
  {
    hex: "d7180e0d0278887f7788788777888787787f88f777",
    first: [168, 147, 119, 255],
    mid: [109, 96, 75, 255],
  },
] as const;

function near(actual: ArrayLike<number>, expected: readonly number[], tolerance = 2) {
  expected.forEach((value, i) => {
    expect(Math.abs((actual[i] ?? -999) - value)).toBeLessThanOrEqual(tolerance);
  });
}

describe("thumbHashFromHex", () => {
  it("reads hex and refuses malformed text", () => {
    expect(thumbHashFromHex("00ff10aabb")).toEqual(new Uint8Array([0, 255, 16, 170, 187]));
    for (const bad of ["", "abc", "zz00ff10aa", "00ff10aab"]) {
      expect(thumbHashFromHex(bad)).toBeNull();
    }
  });
});

describe("decodeThumbHash", () => {
  it.each(SAMPLES)("matches the Rust decoder for $hex", ({ hex, first, mid }) => {
    const hash = thumbHashFromHex(hex);
    expect(hash).not.toBeNull();
    const decoded = decodeThumbHash(hash as Uint8Array);
    expect(decoded).not.toBeNull();
    const { width, height, rgba } = decoded as NonNullable<typeof decoded>;
    expect([width, height]).toEqual([23, 32]);
    near(rgba.slice(0, 4), first);
    const centre = ((height >> 1) * width + (width >> 1)) * 4;
    near(rgba.slice(centre, centre + 4), mid);
  });

  it("reports a portrait aspect ratio for poster hashes and refuses truncated input", () => {
    const hash = thumbHashFromHex(SAMPLES[0].hex) as Uint8Array;
    expect(thumbHashAspectRatio(hash)).toBeLessThan(1);
    expect(decodeThumbHash(new Uint8Array([1, 2]))).toBeNull();
  });
});
