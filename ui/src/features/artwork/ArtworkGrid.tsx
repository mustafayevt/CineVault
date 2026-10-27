import { Check } from "lucide-react";
import { useEffect, useRef } from "react";
import type { MouseEvent } from "react";

import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import { activeLocale } from "../../i18n/locale";
import { cx } from "../../lib/cx";
import { DetailImage } from "../detail/DetailImage";
import { artworkLanguageLabel } from "./artworkLabels";
import type { ArtworkCandidate } from "./artworkBackend";
import { dimensions, languageOf, slotAspect, slotFitsWhole } from "./artworkModel";
import { sourceLine } from "./gridLayout";

/** The short language mark on a tile: `EN`, `TR`, `Textless`. */
function languageMark(candidate: ArtworkCandidate): string {
  const code = languageOf(candidate);
  return code === "none" ? t("artworkPicker.language.textless") : code.toUpperCase();
}

interface ArtworkTileProps {
  id: string;
  candidate: ArtworkCandidate;
  slot: string;
  names: Readonly<Record<string, string>>;
  current: boolean;
  chosen: boolean;
  /** One line only (the narrow batch grid). */
  compact?: boolean;
  onClick: (event: MouseEvent) => void;
  onDoubleClick: (event: MouseEvent) => void;
}

/**
 * One image of the grid: the picture at its real shape, a
 * Current tag on the image the title has now, a gold ring and check on the chosen one, then the
 * size with its language mark and the provider with what people think of it.
 */
export function ArtworkTile({
  id,
  candidate,
  slot,
  names,
  current,
  chosen,
  compact = false,
  onClick,
  onDoubleClick,
}: ArtworkTileProps) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (chosen) ref.current?.scrollIntoView({ block: "nearest" });
  }, [chosen]);
  const size = dimensions(candidate);
  const language = artworkLanguageLabel(languageOf(candidate), activeLocale());
  return (
    // The grid holds the focus and points at the chosen tile (aria-activedescendant).
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/interactive-supports-focus
    <div
      ref={ref}
      id={id}
      role="option"
      aria-selected={chosen}
      aria-label={`${current ? `${t("artworkPicker.current")}, ` : ""}${size}, ${language}`}
      data-art-candidate={candidate.id}
      data-current={current ? "true" : undefined}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      className="flex min-w-0 cursor-pointer flex-col gap-1.5"
    >
      <div
        style={{ aspectRatio: slotAspect(slot) }}
        className={cx(
          "cv-hoverable relative overflow-hidden rounded-(--r-10) bg-surface-card",
          chosen ? "outline-2 outline-offset-[3px] outline-gold" : "hover:brightness-110",
        )}
      >
        <DetailImage
          image={candidate.image}
          size="grid"
          className={slotFitsWhole(slot) ? "object-contain" : "object-cover"}
        />
        {current ? (
          <Tag className="absolute top-1.5 left-1.5 bg-[color-mix(in_srgb,var(--surface-window)_82%,transparent)] text-text">
            {t("artworkPicker.current")}
          </Tag>
        ) : null}
        {chosen ? (
          <span
            aria-hidden="true"
            className="absolute top-1.5 right-1.5 flex size-5 items-center justify-center rounded-(--r-pill) bg-gold text-on-gold"
          >
            <Check size={12} strokeWidth={2.8} />
          </span>
        ) : null}
      </div>
      <div className="flex flex-wrap items-center gap-[5px] text-(length:--fs-12) text-text-2">
        <span className="font-(weight:--fw-500) text-text">{size}</span>
        <Tag>{languageMark(candidate)}</Tag>
        {compact && !current ? (
          <span className="text-text-muted">{sourceLine(candidate, names)}</span>
        ) : null}
      </div>
      {compact || current ? null : (
        <div className="-mt-0.5 truncate text-(length:--fs-12) text-text-muted">
          {sourceLine(candidate, names)}
        </div>
      )}
    </div>
  );
}
