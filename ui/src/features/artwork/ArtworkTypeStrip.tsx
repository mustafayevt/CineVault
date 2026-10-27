import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { DetailImage } from "../detail/DetailImage";
import type { ArtworkCandidate } from "./artworkBackend";
import { typeName } from "./pickerModel";

interface ArtworkTypeStripProps {
  slots: readonly string[];
  active: string;
  onSelect: (slot: string) => void;
  /** How many images each type is offered, once its search has answered. */
  counts: Readonly<Record<string, number | undefined>>;
  /** The image the title has now for each type. */
  currents: Readonly<Record<string, ArtworkCandidate | null | undefined>>;
  width?: number;
}

/**
 * The strip of artwork types: each with a thumbnail of its
 * current image (a dashed box when it has none) and how many images the providers offer. The type
 * on screen takes the active fill and the gold edge.
 */
export function ArtworkTypeStrip({
  slots,
  active,
  onSelect,
  counts,
  currents,
  width = 196,
}: ArtworkTypeStripProps) {
  const format = useFormat();
  return (
    <nav
      aria-label={t("artworkPicker.types")}
      style={{ width }}
      className="flex shrink-0 flex-col gap-0.5 border-r border-solid border-line-section px-2 py-3"
    >
      {slots.map((slot) => {
        const on = slot === active;
        const current = currents[slot];
        const count = counts[slot];
        return (
          <button
            key={slot}
            type="button"
            aria-current={on ? "true" : undefined}
            data-slot={slot}
            onClick={() => onSelect(slot)}
            className={cx(
              "cv-hoverable flex h-10 w-full items-center gap-2.5 rounded-(--r-6) border-0 px-2.5 text-left",
              "text-(length:--fs-13)",
              on
                ? "bg-surface-active font-(weight:--fw-600) text-text shadow-[inset_2px_0_0_var(--gold)]"
                : "bg-transparent font-(weight:--fw-500) text-text-2 hover:bg-surface-card active:bg-surface-raised",
            )}
          >
            <span
              className={cx(
                "relative size-[22px] shrink-0 overflow-hidden rounded-(--r-4)",
                current == null && "border border-dashed border-line-edge",
              )}
            >
              {current == null ? null : (
                <DetailImage image={current.image} size="list" className="object-cover" />
              )}
            </span>
            <span className="min-w-0 flex-1 truncate">{typeName(slot)}</span>
            <span
              className="min-w-[2ch] text-right text-(length:--fs-12) font-normal text-text-muted tabular-nums"
              aria-label={
                count === undefined ? undefined : t("artworkPicker.type.count", { count })
              }
            >
              {count === undefined ? "" : format.number(count)}
            </span>
          </button>
        );
      })}
    </nav>
  );
}
