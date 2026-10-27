import { Link } from "@tanstack/react-router";

import type { DetailArtworkDto } from "../../bindings";
import { SectionLabel } from "../../components/SectionLabel";
import { t } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { DetailImage } from "../detail/DetailImage";
import type { ThumbSize } from "../images/imageUrl";

interface Shape {
  slot: string;
  label: MessageKey;
  width: number;
  height: number;
  /** Shown whole on a checkerboard (transparent art) instead of cropped to fill. */
  whole?: boolean;
  round?: boolean;
  size: ThumbSize;
}

/** Every artwork type with the shape it really has, in a fixed order. */
const SHAPES: readonly Shape[] = [
  { slot: "poster", label: "movie.art.poster", width: 96, height: 144, size: "grid" },
  { slot: "fanart", label: "movie.art.fanart", width: 256, height: 144, size: "hero" },
  {
    slot: "clearlogo",
    label: "movie.art.clearlogo",
    width: 200,
    height: 78,
    whole: true,
    size: "grid",
  },
  {
    slot: "clearart",
    label: "movie.art.clearart",
    width: 140,
    height: 78,
    whole: true,
    size: "grid",
  },
  {
    slot: "disc",
    label: "movie.art.disc",
    width: 96,
    height: 96,
    whole: true,
    round: true,
    size: "grid",
  },
  { slot: "banner", label: "movie.art.banner", width: 210, height: 39, size: "grid" },
  { slot: "keyart", label: "movie.art.keyart", width: 96, height: 144, size: "grid" },
  { slot: "thumb", label: "movie.art.thumb", width: 160, height: 90, size: "grid" },
];

function dimensions(art: DetailArtworkDto | undefined): string | null {
  if (art === undefined || art.image === null) return null;
  return art.width > 0 && art.height > 0 ? `${art.width}×${art.height}` : "";
}

/**
 * The artwork strip of the Movie page: a tile for every type the library writes, at its real
 * shape with its size under it. A type with no image says "Missing" with Choose; both Choose and
 * Change artwork open the artwork picker on that type. The header counts the filled types.
 */
export function ArtworkStrip({
  itemId,
  artwork,
}: {
  itemId: string;
  artwork: readonly DetailArtworkDto[];
}) {
  const bySlot = new Map(artwork.map((art) => [art.slot, art]));
  const filled = SHAPES.filter((shape) => dimensions(bySlot.get(shape.slot)) !== null).length;
  return (
    <section aria-label={t("movie.art.title")} className="px-(--inset) pt-7">
      <SectionLabel
        action={
          <span className="flex items-baseline gap-3.5">
            <span>{t("movie.art.filled", { filled, total: SHAPES.length })}</span>
            <Link to="/artwork" search={{ ids: itemId } as never} className="cv-link">
              {t("movie.art.change")}
            </Link>
          </span>
        }
      >
        {t("movie.art.title")}
      </SectionLabel>
      <div className="flex flex-wrap items-end gap-x-3.5 gap-y-4">
        {SHAPES.map((shape) => {
          const art = bySlot.get(shape.slot);
          const size = dimensions(art);
          const present = art?.image != null;
          const label = t(shape.label);
          return (
            <Link
              key={shape.slot}
              to="/artwork"
              search={{ ids: itemId, type: shape.slot } as never}
              data-art={shape.slot}
              data-state={present ? "filled" : "missing"}
              aria-label={
                present
                  ? t("movie.art.changeOf", { type: label })
                  : t("movie.art.chooseOf", { type: label })
              }
              className="flex flex-col gap-[7px] text-inherit no-underline"
            >
              <span
                style={{ width: shape.width, height: shape.height }}
                className={cx(
                  "cv-lift relative block overflow-hidden",
                  shape.round ? "rounded-(--r-pill)" : "rounded-(--r-10)",
                  present
                    ? "bg-surface-raised"
                    : "box-border flex items-center justify-center border-[1.5px] border-dashed border-line-edge text-(length:--fs-12) text-text-muted",
                )}
              >
                {present && art?.image != null ? (
                  <DetailImage
                    image={art.image}
                    size={shape.size}
                    className={shape.whole === true ? "object-contain" : "object-cover"}
                  />
                ) : (
                  t("movie.art.choose")
                )}
              </span>
              <span className={cx("text-(length:--fs-12)", !present && "text-text-muted")}>
                {label}
              </span>
              <span className="-mt-1.5 text-(length:--fs-11) text-text-muted">
                {present ? size : t("movie.art.missing")}
              </span>
            </Link>
          );
        })}
      </div>
    </section>
  );
}
