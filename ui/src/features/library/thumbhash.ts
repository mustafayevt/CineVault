/**
 * Decodes a ThumbHash (the few-dozen-byte image preview stored with every cached thumbnail) into
 * a small RGBA bitmap. A poster card paints it in its first frame, so a tile is never blank while
 * the real thumbnail loads. The format is the public ThumbHash specification; the Rust side
 * encodes it (`cv_artwork::thumb`).
 */

export interface DecodedThumbHash {
  width: number;
  height: number;
  /** Straight (non-premultiplied) RGBA, `width * height * 4` bytes. */
  rgba: Uint8ClampedArray;
}

/** The ThumbHash bytes from the hex text the backend sends; `null` for malformed text. */
export function thumbHashFromHex(hex: string): Uint8Array | null {
  if (hex.length < 10 || hex.length % 2 !== 0 || !/^[0-9a-f]+$/i.test(hex)) return null;
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i += 1) {
    bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

/** Width over height of the original image, as far as the hash records it. */
export function thumbHashAspectRatio(hash: Uint8Array): number {
  const header = hash[3] ?? 0;
  const hasAlpha = ((hash[2] ?? 0) & 0x80) !== 0;
  const isLandscape = ((hash[4] ?? 0) & 0x80) !== 0;
  const lx = isLandscape ? (hasAlpha ? 5 : 7) : header & 7;
  const ly = isLandscape ? header & 7 : hasAlpha ? 5 : 7;
  return lx / ly;
}

/** Decodes to a bitmap at most 32 pixels on its longer side. */
export function decodeThumbHash(hash: Uint8Array): DecodedThumbHash | null {
  if (hash.length < 5) return null;
  const byte = (i: number): number => hash[i] ?? 0;

  const header24 = byte(0) | (byte(1) << 8) | (byte(2) << 16);
  const header16 = byte(3) | (byte(4) << 8);
  const lDc = (header24 & 63) / 63;
  const pDc = ((header24 >> 6) & 63) / 31.5 - 1;
  const qDc = ((header24 >> 12) & 63) / 31.5 - 1;
  const lScale = ((header24 >> 18) & 31) / 31;
  const hasAlpha = header24 >> 23 !== 0;
  const pScale = ((header16 >> 3) & 63) / 63;
  const qScale = ((header16 >> 9) & 63) / 63;
  const isLandscape = header16 >> 15 !== 0;
  const lx = Math.max(3, isLandscape ? (hasAlpha ? 5 : 7) : header16 & 7);
  const ly = Math.max(3, isLandscape ? header16 & 7 : hasAlpha ? 5 : 7);
  const aDc = hasAlpha ? (byte(5) & 15) / 15 : 1;
  const aScale = (byte(5) >> 4) / 15;

  const acStart = hasAlpha ? 6 : 5;
  let acIndex = 0;
  const decodeChannel = (nx: number, ny: number, scale: number): number[] => {
    const ac: number[] = [];
    for (let cy = 0; cy < ny; cy += 1) {
      for (let cx = cy > 0 ? 0 : 1; cx * ny < nx * (ny - cy); cx += 1) {
        const nibble = (byte(acStart + (acIndex >> 1)) >> ((acIndex & 1) << 2)) & 15;
        acIndex += 1;
        ac.push((nibble / 7.5 - 1) * scale);
      }
    }
    return ac;
  };
  const lAc = decodeChannel(lx, ly, lScale);
  const pAc = decodeChannel(3, 3, pScale * 1.25);
  const qAc = decodeChannel(3, 3, qScale * 1.25);
  const aAc = hasAlpha ? decodeChannel(5, 5, aScale) : [];

  const ratio = thumbHashAspectRatio(hash);
  const width = Math.round(ratio > 1 ? 32 : 32 * ratio);
  const height = Math.round(ratio > 1 ? 32 / ratio : 32);
  if (width < 1 || height < 1) return null;
  const rgba = new Uint8ClampedArray(width * height * 4);
  const fx: number[] = [];
  const fy: number[] = [];
  const columns = Math.max(lx, hasAlpha ? 5 : 3);
  const rows = Math.max(ly, hasAlpha ? 5 : 3);

  for (let y = 0, i = 0; y < height; y += 1) {
    for (let cy = 0; cy < rows; cy += 1) fy[cy] = Math.cos((Math.PI / height) * (y + 0.5) * cy);
    for (let x = 0; x < width; x += 1, i += 4) {
      for (let cx = 0; cx < columns; cx += 1) fx[cx] = Math.cos((Math.PI / width) * (x + 0.5) * cx);
      let l = lDc;
      let p = pDc;
      let q = qDc;
      let a = aDc;

      for (let cy = 0, j = 0; cy < ly; cy += 1) {
        const fy2 = (fy[cy] ?? 0) * 2;
        for (let cx = cy > 0 ? 0 : 1; cx * ly < lx * (ly - cy); cx += 1, j += 1) {
          l += (lAc[j] ?? 0) * (fx[cx] ?? 0) * fy2;
        }
      }
      for (let cy = 0, j = 0; cy < 3; cy += 1) {
        const fy2 = (fy[cy] ?? 0) * 2;
        for (let cx = cy > 0 ? 0 : 1; cx < 3 - cy; cx += 1, j += 1) {
          const f = (fx[cx] ?? 0) * fy2;
          p += (pAc[j] ?? 0) * f;
          q += (qAc[j] ?? 0) * f;
        }
      }
      if (hasAlpha) {
        for (let cy = 0, j = 0; cy < 5; cy += 1) {
          const fy2 = (fy[cy] ?? 0) * 2;
          for (let cx = cy > 0 ? 0 : 1; cx < 5 - cy; cx += 1, j += 1) {
            a += (aAc[j] ?? 0) * (fx[cx] ?? 0) * fy2;
          }
        }
      }

      const b = l - (2 / 3) * p;
      const r = (3 * l - b + q) / 2;
      const g = r - q;
      rgba[i] = Math.max(0, 255 * Math.min(1, r));
      rgba[i + 1] = Math.max(0, 255 * Math.min(1, g));
      rgba[i + 2] = Math.max(0, 255 * Math.min(1, b));
      rgba[i + 3] = Math.max(0, 255 * Math.min(1, a));
    }
  }
  return { width, height, rgba };
}
