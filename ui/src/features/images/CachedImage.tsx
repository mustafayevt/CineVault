import { useCallback, useState } from "react";
import type { ImgHTMLAttributes, ReactEventHandler } from "react";

import { cx } from "../../lib/cx";
import { ThumbHashCanvas } from "../library/ThumbHashCanvas";
import { imageUrl } from "./imageUrl";
import type { ThumbSize } from "./imageUrl";

interface CachedImageProps extends Omit<ImgHTMLAttributes<HTMLImageElement>, "src" | "alt"> {
  /** Content key of the image (see the thumbnail cache). */
  imageKey: string;
  size: ThumbSize;
  /** Describes the image; pass an empty string only for decoration. */
  alt: string;
  /** The ThumbHash as hex: painted first, under the image, until it has loaded. */
  thumbhash?: string | null;
  /** The ambient colour as `#rrggbb`: the placeholder when there is no ThumbHash. */
  ambient?: string | null;
  /** Classes of the frame (size, position and shape), not of the image inside it. */
  className?: string;
}

/**
 * An image served by the `cv://img` protocol. The browser loads and caches it natively, lazily
 * (only near the viewport) and off the main thread; no image data passes through the app's
 * JavaScript. The frame first shows the ThumbHash or ambient-colour placeholder, and the image
 * crossfades in over `--dur-page` once it has loaded.
 */
export function CachedImage({
  imageKey,
  size,
  alt,
  thumbhash = null,
  ambient = null,
  className,
  onLoad,
  style,
  ...rest
}: CachedImageProps) {
  const [loaded, setLoaded] = useState(false);
  // A cached image can be complete before React attaches onLoad, so check on mount.
  const attach = useCallback((img: HTMLImageElement | null) => {
    if (img?.complete === true && img.naturalWidth > 0) setLoaded(true);
  }, []);
  const handleLoad: ReactEventHandler<HTMLImageElement> = (event) => {
    setLoaded(true);
    onLoad?.(event);
  };
  return (
    <span
      data-testid="cached-image"
      data-loaded={loaded}
      className={cx("bg-surface-raised grid overflow-hidden", className)}
      style={ambient !== null && thumbhash === null ? { backgroundColor: ambient } : undefined}
    >
      {thumbhash !== null && !loaded && (
        <ThumbHashCanvas hash={thumbhash} className="col-start-1 row-start-1 size-full" />
      )}
      <img
        {...rest}
        ref={attach}
        src={imageUrl(imageKey, size)}
        alt={alt}
        loading="lazy"
        decoding="async"
        draggable={false}
        onLoad={handleLoad}
        style={style}
        className={cx(
          "col-start-1 row-start-1 block size-full object-cover",
          "transition-opacity duration-(--dur-page) ease-(--ease-out)",
          loaded ? "opacity-100" : "opacity-0",
        )}
      />
    </span>
  );
}
