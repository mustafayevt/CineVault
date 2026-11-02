import { FileImage, Maximize2 } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";

import { Icon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { SectionLabel } from "../../components/SectionLabel";
import { t } from "../../i18n/catalog";
import { activeLocale } from "../../i18n/locale";
import { cx } from "../../lib/cx";
import { DetailImage } from "../detail/DetailImage";
import { artworkLanguageLabel, popularityWords } from "./artworkLabels";
import type { ArtworkCandidate } from "./artworkBackend";
import type { ArtworkChoice } from "./artworkApply";
import { dimensions, languageOf, popularityOf, slotAspect, slotFitsWhole } from "./artworkModel";
import { providerLabel, savedAs, typeName } from "./pickerModel";

interface ArtworkPreviewProps {
  slot: string;
  /** What the user chose, or `null` while nothing is: the current image shows then. */
  chosen: ArtworkChoice | null;
  /** The image the title has now for the type. */
  current: ArtworkCandidate | null;
  names: Readonly<Record<string, string>>;
  /** The buttons at the foot (Use this, Cancel; or the stepping buttons). */
  actions: ReactNode;
  /** The panel's width: pixels, or a CSS length such as a clamp(). */
  width?: number | string;
  /** The heading over the image: "Preview", "Preview · Fanart". */
  heading?: string;
  /** Opens the picture large; the expand button shows on hover and focus while there is one. */
  onExpand?: () => void;
}

/** The image of a choice at its real shape, however it came. */
function Picture({
  slot,
  chosen,
  fallback,
  onExpand,
}: {
  slot: string;
  chosen: ArtworkChoice | null;
  fallback: ArtworkCandidate | null;
  onExpand?: (() => void) | undefined;
}) {
  const whole = slotFitsWhole(slot);
  const tall = slotAspect(slot) === "2 / 3";
  const frame = cx(
    "group/picture relative overflow-hidden rounded-(--r-10) bg-surface-well shadow-floating",
    tall ? "h-[clamp(220px,calc(100vh-430px),480px)] self-center" : "w-full",
    // Narrow panes size the picture by the width it has.
    "@max-[860px]/pane:h-auto @max-[860px]/pane:w-full @max-[640px]/pane:self-start",
  );
  const style = { aspectRatio: slotAspect(slot) };
  // Shown while the pointer is over the picture or the button has the keyboard focus.
  const expand =
    onExpand === undefined ? null : (
      <IconButton
        icon={Maximize2}
        size="sm"
        aria-label={t("artworkPicker.expand")}
        onClick={onExpand}
        className={cx(
          "absolute top-2 right-2 z-10 bg-surface-floating opacity-0 transition-opacity duration-(--dur-hover)",
          "group-hover/picture:opacity-100 focus-visible:opacity-100",
          // No hover on a touch screen: narrow panes always show it.
          "@max-[860px]/pane:opacity-100",
        )}
      />
    );
  if (chosen?.kind === "candidate" || (chosen === null && fallback !== null)) {
    const candidate =
      chosen?.kind === "candidate" ? chosen.candidate : (fallback as ArtworkCandidate);
    return (
      <div style={style} className={frame} data-testid="art-preview">
        <DetailImage
          image={candidate.image}
          size="hero"
          className={whole ? "object-contain" : "object-cover"}
        />
        {expand}
      </div>
    );
  }
  const url = chosen?.kind === "file" ? chosen.url : chosen?.kind === "link" ? chosen.url : null;
  if (url !== null && url !== undefined) {
    return (
      <div style={style} className={frame} data-testid="art-preview">
        <img
          src={url}
          alt=""
          draggable={false}
          className={cx("absolute inset-0 size-full", whole ? "object-contain" : "object-cover")}
        />
        {expand}
      </div>
    );
  }
  return (
    <div
      style={style}
      className={cx(frame, "flex flex-col items-center justify-center gap-2 p-4 text-center")}
      data-testid="art-preview"
    >
      <Icon icon={FileImage} size={24} className="text-text-muted" />
      <p className="m-0 text-text-muted">{t("artworkPicker.noPreview")}</p>
    </div>
  );
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-text-muted">{label}</dt>
      <dd className="m-0 min-w-0 break-words">{children}</dd>
    </>
  );
}

/**
 * The large preview of the chosen image: the picture at its
 * real shape, its size, language, source and the file name it is saved as, and the image the title
 * has now beside it. A flat docked pane on the card colour; nothing is written until its buttons.
 */
export function ArtworkPreview({
  slot,
  chosen,
  current,
  names,
  actions,
  width = 420,
  heading,
  onExpand,
}: ArtworkPreviewProps) {
  const locale = activeLocale();
  const candidate = chosen?.kind === "candidate" ? chosen.candidate : null;
  const shown = chosen === null ? current : candidate;
  const provider = shown === null ? "" : providerLabel(shown, names);
  const popularity = candidate === null ? null : popularityOf(candidate);
  const type = typeName(slot).toLowerCase();
  return (
    <aside
      aria-label={t("artworkPicker.preview")}
      style={{ "--preview-w": typeof width === "number" ? `${width}px` : width } as CSSProperties}
      className={cx(
        "flex w-(--preview-w) shrink-0 flex-col gap-3.5 overflow-y-auto border-l border-solid border-line-section bg-surface-card px-6 py-[18px]",
        "@max-[860px]/pane:w-full",
        // A phone shows it under the images: the picture small beside its facts, then the buttons.
        "@max-[640px]/pane:grid @max-[640px]/pane:grid-cols-[88px_minmax(0,1fr)] @max-[640px]/pane:content-start @max-[640px]/pane:gap-x-3.5 @max-[640px]/pane:gap-y-3 @max-[640px]/pane:overflow-visible @max-[640px]/pane:border-t @max-[640px]/pane:border-l-0 @max-[640px]/pane:px-4 @max-[640px]/pane:py-3",
      )}
    >
      <SectionLabel className="mb-0 @max-[640px]/pane:hidden">
        {heading ?? t("artworkPicker.preview")}
      </SectionLabel>
      <Picture slot={slot} chosen={chosen} fallback={current} onExpand={onExpand} />
      <dl className="m-0 grid grid-cols-[96px_minmax(0,1fr)] gap-y-[5px] @max-[640px]/pane:grid-cols-[72px_minmax(0,1fr)] @max-[640px]/pane:self-start @max-[640px]/pane:text-(length:--fs-12)">
        {shown === null && chosen === null ? (
          <p className="col-span-2 m-0 text-text-muted">{t("artworkPicker.preview.none")}</p>
        ) : null}
        {shown !== null ? (
          <>
            <Fact label={t("artworkPicker.fact.size")}>
              {dimensions(shown) || t("artworkPicker.fact.sizeUnknown")}
            </Fact>
            <Fact label={t("artworkPicker.fact.language")}>
              {artworkLanguageLabel(languageOf(shown), locale)}
            </Fact>
            <Fact label={t("artworkPicker.fact.source")}>
              {provider}
              {popularity === null ? null : ` · ${popularityWords(popularity, locale)}`}
            </Fact>
          </>
        ) : null}
        {chosen?.kind === "file" ? (
          <>
            <Fact label={t("artworkPicker.fact.source")}>{t("artworkPicker.source.file")}</Fact>
            <Fact label={t("artworkPicker.fact.size")}>{chosen.name}</Fact>
          </>
        ) : null}
        {chosen?.kind === "link" ? (
          <Fact label={t("artworkPicker.fact.source")}>
            {t("artworkPicker.source.link")} · <span className="cv-selectable">{chosen.url}</span>
          </Fact>
        ) : null}
        {chosen !== null ? (
          <Fact label={t("artworkPicker.fact.savedAs")}>
            <span className="cv-selectable font-mono text-(length:--fs-12)">
              {savedAs(slot, chosen.kind === "file" ? chosen.name : undefined)}
            </span>
          </Fact>
        ) : null}
      </dl>
      <div className="grow @max-[640px]/pane:hidden" />
      <div className="flex items-center gap-2.5 text-(length:--fs-12) text-text-muted @max-[640px]/pane:hidden">
        <span className="relative h-[33px] w-[22px] shrink-0 overflow-hidden rounded-(--r-4) border border-dashed border-line-edge">
          {current === null ? null : (
            <DetailImage image={current.image} size="list" className="object-cover" />
          )}
        </span>
        {current === null
          ? t("artworkPicker.current.none", { type })
          : t("artworkPicker.current.of", { type: typeName(slot).toLowerCase() })}
      </div>
      <div className="flex gap-2 @max-[860px]/pane:flex-wrap @max-[640px]/pane:col-span-2">
        {actions}
      </div>
    </aside>
  );
}
