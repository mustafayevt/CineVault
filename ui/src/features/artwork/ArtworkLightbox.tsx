import { LoaderCircle } from "lucide-react";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";

import { Icon } from "../../components/Icon";
import { useFloatingContainer } from "../../components/floating";
import { useReducedMotion } from "../../components/motionHooks";
import { t } from "../../i18n/catalog";
import type { DetailImageRef } from "../detail/detailBackend";
import { imageUrl } from "../images/imageUrl";
import { useFullImage } from "./useFullImage";

/** Pixels of a picture. */
export interface Pixels {
  width: number;
  height: number;
}

/** How long the full picture takes to fade in over the thumbnail (`--dur-pop`). */
const FADE_MS = 140;

/** "2 / 3" as 0.667. */
function ratioOf(aspect: string): number {
  const [width, height] = aspect.split("/").map((part) => Number(part.trim()));
  return width !== undefined && height !== undefined && height > 0 ? width / height : 1;
}

interface ArtworkLightboxProps {
  /**
   * What is shown: a candidate (its `id` is what the backend fetches the original by, `image` is
   * its cached thumbnail, `neighbours` the ids of the images a Left or Right shows next), or a
   * picture of the user by its address.
   */
  image:
    | { kind: "candidate"; id: string; image: DetailImageRef; neighbours?: readonly string[] }
    | { kind: "url"; url: string };
  /**
   * "Poster · 1000×1500 · TMDB", under the picture; also its name for screen readers. It gets
   * the real size of the picture once the original has loaded, `null` before.
   */
  caption: (pixels: Pixels | null) => string;
  /** The shape of the type ("2 / 3"), so the picture has its place before it has loaded. */
  aspect: string;
  onClose: () => void;
  /** Left and Right flick to the neighbouring image; without it the arrow keys do nothing. */
  onStep?: (delta: -1 | 1) => void;
}

/**
 * A big look at one picture of the artwork picker, over the dimmed page: about 90% of the window,
 * the image fitted inside with its type, size and source under it. A candidate shows its
 * thumbnail at once, scaled to fit; the original is fetched into the image cache meanwhile (a
 * small spinner says so) and fades over it when it has loaded, and the caption then gives its
 * real size. When it cannot be fetched the thumbnail stays, with a note. It only shows: nothing
 * is chosen or written. Any click, or Esc, closes it; Left and Right step through the images of
 * the type. It fades in and grows over `--dur-pop` (at once with reduced motion), and keeps the
 * focus where it was, so the grid still answers the keys afterwards.
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

  const candidate = image.kind === "candidate" ? image : null;
  const full = useFullImage(candidate?.id ?? null, candidate?.neighbours ?? []);
  // Which original has finished loading (its size is read from the loaded picture) and which one
  // failed to load. Both are tied to a key, so a flick to another image starts clean.
  const [loaded, setLoaded] = useState<(Pixels & { key: string }) | null>(null);
  const [broken, setBroken] = useState<string | null>(null);
  const fullKey = full.status === "ready" ? full.key : null;
  const ready = fullKey !== null && loaded?.key === fullKey ? loaded : null;
  const failed =
    candidate !== null && (full.status === "failed" || (fullKey !== null && broken === fullKey));
  // The thumbnail goes once the original covers it, so a transparent logo is not drawn twice.
  const [covered, setCovered] = useState<string | null>(null);
  const readyKey = ready?.key ?? null;
  useEffect(() => {
    if (readyKey === null) return undefined;
    const timer = setTimeout(() => setCovered(readyKey), reduced ? 0 : FADE_MS + 20);
    return () => clearTimeout(timer);
  }, [readyKey, reduced]);
  const thumbGone = readyKey !== null && covered === readyKey;
  const loading = candidate !== null && !failed && ready === null;

  const text = caption(ready);
  const still = reduced ? { animation: "none" } : undefined;
  return createPortal(
    <div
      role="group"
      aria-label={text}
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
          className="relative overflow-hidden rounded-(--r-10) bg-surface-well shadow-floating"
        >
          {candidate === null ? (
            <img
              src={image.kind === "url" ? image.url : ""}
              alt={text}
              draggable={false}
              className="size-full object-contain"
            />
          ) : (
            <>
              <img
                src={imageUrl(candidate.image.key, "grid")}
                alt={text}
                draggable={false}
                data-layer="thumbnail"
                className="absolute inset-0 size-full object-contain"
                style={thumbGone ? { visibility: "hidden" } : undefined}
              />
              {fullKey !== null && !failed ? (
                <img
                  key={fullKey}
                  src={imageUrl(fullKey, "full")}
                  alt=""
                  draggable={false}
                  data-layer="original"
                  onLoad={(event) =>
                    setLoaded({
                      key: fullKey,
                      width: event.currentTarget.naturalWidth,
                      height: event.currentTarget.naturalHeight,
                    })
                  }
                  onError={() => setBroken(fullKey)}
                  className={`absolute inset-0 size-full object-contain ${
                    reduced ? "" : "transition-opacity duration-(--dur-pop) ease-out"
                  } ${ready !== null ? "opacity-100" : "opacity-0"}`}
                />
              ) : null}
              {loading ? (
                <span
                  role="status"
                  aria-label={t("artworkPicker.lightbox.loading")}
                  className="absolute right-2.5 bottom-2.5 grid size-7 place-items-center rounded-full bg-scrim text-scrim-text"
                >
                  <Icon icon={LoaderCircle} size={16} className="animate-spin" />
                </span>
              ) : null}
            </>
          )}
        </div>
        <figcaption className="text-(length:--fs-13) text-scrim-text">
          {text}
          {failed ? ` · ${t("artworkPicker.lightbox.failed")}` : ""}
        </figcaption>
      </figure>
      <span className="sr-only">{t("artworkPicker.lightbox.hint")}</span>
    </div>,
    container ?? document.body,
  );
}
