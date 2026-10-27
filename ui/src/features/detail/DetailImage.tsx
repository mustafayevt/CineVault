import { useEffect, useState } from "react";

import { cx } from "../../lib/cx";
import { imageUrl } from "../images/imageUrl";
import type { ThumbSize } from "../images/imageUrl";
import { ThumbHashCanvas } from "../library/ThumbHashCanvas";
import type { DetailImageRef } from "./detailBackend";

interface DetailImageProps {
  image: DetailImageRef;
  size: ThumbSize;
  /** Describes the image; empty when the caption next to it already names it. */
  alt?: string;
  className?: string;
  /** Told the image's own size once it has loaded (the hero checks a clear logo's shape). */
  onNaturalSize?: (width: number, height: number) => void;
}

/**
 * How long after loading the preview goes: a little longer than the `--dur-page` fade, so
 * an opaque image has covered it by then.
 */
const SETTLE_MS = 300;

/**
 * An image of the detail page: the ThumbHash preview is painted first, and the thumbnail (served
 * by `cv://img`, never through IPC) fades in over it once it has loaded. The preview is then
 * removed, so a transparent image (a clear logo) does not sit on a blurred box. An image that
 * cannot be loaded leaves the preview in place instead of a broken-image icon.
 */
export function DetailImage({ image, size, alt = "", className, onNaturalSize }: DetailImageProps) {
  const [state, setState] = useState<"loading" | "loaded" | "settled" | "failed">("loading");
  useEffect(() => {
    if (state !== "loaded") return;
    const timer = window.setTimeout(() => setState("settled"), SETTLE_MS);
    return () => window.clearTimeout(timer);
  }, [state]);
  return (
    <>
      {image.thumbhash !== null && state !== "settled" && (
        <ThumbHashCanvas
          hash={image.thumbhash}
          className={cx("absolute inset-0 size-full", className)}
        />
      )}
      {state !== "failed" && (
        <img
          src={imageUrl(image.key, size, { preview: image.preview === true })}
          alt={alt}
          loading="lazy"
          decoding="async"
          draggable={false}
          onLoad={(event) => {
            setState("loaded");
            onNaturalSize?.(event.currentTarget.naturalWidth, event.currentTarget.naturalHeight);
          }}
          onError={() => setState("failed")}
          className={cx(
            "absolute inset-0 size-full transition-opacity duration-(--dur-page) ease-(--ease-out)",
            state === "loaded" || state === "settled" ? "opacity-100" : "opacity-0",
            className,
          )}
        />
      )}
    </>
  );
}
