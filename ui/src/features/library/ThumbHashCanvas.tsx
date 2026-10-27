import { useLayoutEffect, useRef } from "react";

import { decodeThumbHash, thumbHashFromHex } from "./thumbhash";
import type { DecodedThumbHash } from "./thumbhash";

/**
 * Decoded previews, remembered by hash so a poster that scrolls back into view costs a canvas
 * paint and no decoding. Bounded: the oldest previews are dropped past {@link MAX_ENTRIES}.
 */
const MAX_ENTRIES = 4000;
const decoded = new Map<string, DecodedThumbHash | null>();

function previewOf(hex: string): DecodedThumbHash | null {
  const known = decoded.get(hex);
  if (known !== undefined) return known;
  const hash = thumbHashFromHex(hex);
  const preview = hash === null ? null : decodeThumbHash(hash);
  if (decoded.size >= MAX_ENTRIES) {
    const oldest = decoded.keys().next();
    if (oldest.done !== true) decoded.delete(oldest.value);
  }
  decoded.set(hex, preview);
  return preview;
}

interface ThumbHashCanvasProps {
  /** The ThumbHash as hex, as the backend sends it. */
  hash: string;
  className?: string;
}

/**
 * Paints a poster's ThumbHash into a tiny canvas that CSS scales up (smoothly, which is what makes
 * it read as a blurred preview). It is drawn in the first frame, under the real thumbnail, so a
 * tile is never empty while its image loads. A canvas needs no PNG encoding, unlike a data URL.
 */
export function ThumbHashCanvas({ hash, className }: ThumbHashCanvasProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  useLayoutEffect(() => {
    const canvas = ref.current;
    const preview = previewOf(hash);
    if (canvas === null || preview === null) return;
    let context: CanvasRenderingContext2D | null = null;
    try {
      context = canvas.getContext("2d");
    } catch {
      // No canvas (a locked-down context): the tile keeps its colour instead.
    }
    if (context === null) return;
    canvas.width = preview.width;
    canvas.height = preview.height;
    const image = context.createImageData(preview.width, preview.height);
    image.data.set(preview.rgba);
    context.putImageData(image, 0, 0);
  }, [hash]);
  return <canvas ref={ref} aria-hidden="true" className={className} />;
}
