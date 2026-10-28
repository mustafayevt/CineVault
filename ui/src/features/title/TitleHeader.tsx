import { useNavigate } from "@tanstack/react-router";
import { Image as ImageIcon } from "lucide-react";
import type { ReactNode } from "react";

import type { LifecycleDto, PosterDto } from "../../bindings";
import { Icon } from "../../components/Icon";
import { PlaceholderBackdrop, PlaceholderPoster } from "../../components/PlaceholderPoster";
import { Tag } from "../../components/Tag";
import { Tooltip } from "../../components/Tooltip";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { DetailImage } from "../detail/DetailImage";
import { TitleBackdrop } from "./TitleBackdrop";
import "./title.css";

interface TitleHeaderProps {
  /** The title's id: the poster and backdrop buttons open the artwork picker for it. */
  itemId: string;
  title: string;
  poster: PosterDto | null;
  backdrop: PosterDto | null;
  lifecycle: LifecycleDto | null;
  /** The muted words after the stage tag: the collection, or "TV show · returning series". */
  context?: ReactNode;
  /** The line under the title: year, certification, runtime, genres. */
  meta: string;
  tagline?: string;
  ratings?: ReactNode;
  /** The action row (`TitleActionBar`). */
  actions: ReactNode;
  /** Facts at the right of the header (the show's). */
  aside?: ReactNode;
  /** Poster width in px: 172 on the Movie page, 140 on the show page. */
  posterWidth?: number;
  /** Height of the backdrop band in px. */
  backdropHeight?: number;
}

/** The stage of a title as the tag shows it: a warning flag wins over the stage word. */
function stageTag(lifecycle: LifecycleDto | null): {
  label: string;
  variant: "ok" | "warn" | "neutral";
} {
  const flags = lifecycle?.flags ?? [];
  if (flags.some((flag) => flag.kind === "not-matched")) {
    return { label: t("stage.flag.notMatched"), variant: "warn" };
  }
  if (flags.some((flag) => flag.kind === "needs-review")) {
    return { label: t("stage.flag.needsReview"), variant: "warn" };
  }
  switch (lifecycle?.stage) {
    case "renamed":
      return { label: t("stage.renamed"), variant: "ok" };
    case "scraped":
      return { label: t("stage.scraped"), variant: "neutral" };
    default:
      return { label: t("stage.new"), variant: "neutral" };
  }
}

interface ArtButtonProps {
  /** Which hover area shows it: the poster, or the whole header over the backdrop band. */
  reveal: "poster" | "backdrop";
  label: string;
  onClick: () => void;
  /** Shows the label beside the icon (the backdrop's); the poster's is too small for text. */
  withText?: boolean;
}

/**
 * A small floating "Change poster" / "Change backdrop" button: hidden until the pointer is over
 * its artwork (CSS in title.css) or it has keyboard focus; opens the artwork picker on that type.
 */
function ArtButton({ reveal, label, onClick, withText = false }: ArtButtonProps) {
  const button = (
    <button
      type="button"
      aria-label={label}
      data-art-button={reveal}
      onClick={onClick}
      className={cx(
        "cv-art-btn inline-flex h-7 cursor-pointer items-center justify-center gap-1.5 rounded-(--r-6) text-(length:--fs-12) font-medium",
        withText ? "px-2.5" : "w-7",
      )}
    >
      <Icon icon={ImageIcon} size={14} />
      {withText ? <span>{label}</span> : null}
    </button>
  );
  // A labelled button needs no tooltip repeating its label.
  return withText ? (
    button
  ) : (
    <Tooltip content={label} side="top">
      {button}
    </Tooltip>
  );
}

/**
 * The header of a title page: the artwork band fading into the window
 * ground, the poster, the stage tag and context line, the title, the meta line, the tagline, the
 * ratings and the actions. Padding --title-lead/24/26 (the lead grows with the window height so the
 * artwork gets room above the poster), 28px between the poster and the text.
 */
export function TitleHeader({
  itemId,
  title,
  poster,
  backdrop,
  lifecycle,
  context,
  meta,
  tagline,
  ratings,
  actions,
  aside,
  posterWidth = 172,
  backdropHeight = 400,
}: TitleHeaderProps) {
  const tag = stageTag(lifecycle);
  const navigate = useNavigate();
  const changeArt = (type: "poster" | "fanart") =>
    void navigate({ to: "/artwork", search: { ids: itemId, type } as never });
  return (
    <header aria-label={title} className="cv-title-header relative">
      <TitleBackdrop height={backdropHeight}>
        <div className="absolute inset-0">
          {backdrop === null ? (
            <PlaceholderBackdrop title={title} />
          ) : (
            <DetailImage
              image={backdrop}
              size="full"
              className="object-cover object-[center_30%]"
            />
          )}
        </div>
        <div className="cv-title-backdrop-fade" />
      </TitleBackdrop>
      <span className="absolute top-3 right-(--inset) z-10">
        <ArtButton
          reveal="backdrop"
          label={t("title.art.changeBackdrop")}
          onClick={() => changeArt("fanart")}
          withText
        />
      </span>
      <div className="relative flex items-end gap-7 px-(--inset) pt-(--title-lead) pb-[26px]">
        <div
          data-testid="title-poster"
          style={{ width: posterWidth }}
          className="relative aspect-[2/3] shrink-0 overflow-hidden rounded-(--r-10) bg-surface-raised shadow-floating"
        >
          {poster === null ? (
            <PlaceholderPoster title={title} />
          ) : (
            <DetailImage image={poster} size="hero" className="object-cover" />
          )}
          <span className="absolute right-2 bottom-2">
            <ArtButton
              reveal="poster"
              label={t("title.art.changePoster")}
              onClick={() => changeArt("poster")}
            />
          </span>
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-2.5 pb-0.5">
          <div className="flex items-center gap-2.5">
            <Tag variant={tag.variant}>{tag.label}</Tag>
            {context === undefined ? null : (
              <span className="text-(length:--fs-12) font-(weight:--fw-500) text-text">{context}</span>
            )}
          </div>
          <h1 className="type-hero m-0 leading-[1.1]">{title}</h1>
          {meta === "" ? null : <div className="text-(length:--fs-14) text-text-2">{meta}</div>}
          {tagline === undefined || tagline === "" ? null : (
            <div className="text-text-muted italic">{tagline}</div>
          )}
          {ratings === undefined ? null : <div className="mt-1">{ratings}</div>}
          <div className="mt-2.5">{actions}</div>
        </div>
        {aside === undefined ? null : <div className="w-[300px] shrink-0 pb-1">{aside}</div>}
      </div>
    </header>
  );
}
