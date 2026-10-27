import { useEffect } from "react";
import { createPortal } from "react-dom";

import { useFloatingContainer } from "../../components/floating";
import { useReducedMotion } from "../../components/motionHooks";
import { t } from "../../i18n/catalog";
import type { DetailImageRef } from "../detail/detailBackend";
import { imageUrl } from "../images/imageUrl";

/** "2 / 3" as 0.667. */
function ratioOf(aspect: string): number {
  const [width, height] = aspect.split("/").map((part) => Number(part.trim()));
  return width !== undefined && height !== undefined && height > 0 ? width / height : 1;
}

interface ArtworkLightboxProps {
  /** What is shown: a candidate's image, or a picture of the user's by its address. */
  image: { kind: "candidate"; image: DetailImageRef } | { kind: "url"; url: string };
  /** "Poster · 1000×1500 · TMDB", under the picture; also its name for screen readers. */
  caption: string;
  /** The shape of the type ("2 / 3"), so the picture has its place before it has loaded. */
  aspect: string;
  onClose: () => void;
  /** Left and Right flick to the neighbouring image; without it the arrow keys do nothing. */
  onStep?: (delta: -1 | 1) => void;
}

/**
 * A big look at one picture of the artwork picker, over the dimmed page: about 90% of the window,
 * the image fitted inside with its type, size and source under it. It only shows: nothing is
 * chosen or written. Any click, or Esc, closes it; Left and Right step through the images of the
 * type. It fades in and grows over `--dur-pop` (at once with reduced motion), and keeps the focus
 * where it was, so the grid still answers the keys afterwards.
 */
export function ArtworkLightbox({ image, caption, aspect, onClose, onStep }: ArtworkLightboxProps) {
  const container = useFloatingContainer();
  const reduced = useReducedMotion();

  // Keys and clicks are taken before the page sees them: Esc closes this and not the picker, and
  // the arrow keys flick here instead of moving the grid's choice.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onClose();
      } else if (
        (event.key === "ArrowLeft" || event.key === "ArrowRight") &&
        onStep !== undefined
      ) {
        event.preventDefault();
        event.stopPropagation();
        onStep(event.key === "ArrowLeft" ? -1 : 1);
      }
    };
    // Any click, on the picture, the dimmed page or the page behind, closes it.
    const onClick = (event: MouseEvent) => {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("click", onClick, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("click", onClick, true);
    };
  }, [onClose, onStep]);

  // Focus is not moved into the overlay; remember it and put it back, in case the open picture
  // took it (the grid is what answers the keys again).
  useEffect(() => {
    const before = document.activeElement;
    return () => {
      if (before instanceof HTMLElement && before.isConnected) before.focus();
    };
  }, []);

  const src = image.kind === "url" ? image.url : imageUrl(image.image.key, "full");
  const still = reduced ? { animation: "none" } : undefined;
  return createPortal(
    <div
      role="group"
      aria-label={caption}
      data-testid="art-lightbox"
      style={still}
      className="cv-xfade fixed inset-0 z-50 flex items-center justify-center bg-scrim p-[5vh] select-none"
    >
      <figure
        style={still}
        className="cv-pop m-0 flex max-h-[90vh] max-w-[90vw] flex-col items-center gap-2.5"
      >
        {/* As tall as fits, in the shape of the type, so it does not collapse while it loads. */}
        <div
          style={{
            aspectRatio: aspect,
            width: `min(90vw, calc((90vh - 44px) * ${ratioOf(aspect)}))`,
          }}
          className="overflow-hidden rounded-(--r-10) bg-surface-well shadow-floating"
        >
          <img src={src} alt={caption} draggable={false} className="size-full object-contain" />
        </div>
        <figcaption className="text-(length:--fs-13) text-scrim-text">{caption}</figcaption>
      </figure>
      <span className="sr-only">{t("artworkPicker.lightbox.hint")}</span>
    </div>,
    container ?? document.body,
  );
}
