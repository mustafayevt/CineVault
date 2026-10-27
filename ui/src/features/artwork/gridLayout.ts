import { useEffect, useRef, useState } from "react";

import { t } from "../../i18n/catalog";
import { providerName } from "../workspaces/providerNames";
import type { ArtworkCandidate } from "./artworkBackend";
import { FRAMES_PROVIDER, popularityOf, slotAspect } from "./artworkModel";

/** The narrowest a tile of a type gets in pixels, so the grid fits as many as the width allows. */
export function tileMinWidth(slot: string): number {
  const aspect = slotAspect(slot);
  if (aspect === "2 / 3" || aspect === "1 / 1") return 125;
  return slot === "banner" ? 320 : 220;
}

const GAP_X = 14;

/** How many tiles of at least `min` pixels fit a width, for the arrow keys to move by rows. */
export function useGridColumns(min: number) {
  const ref = useRef<HTMLDivElement>(null);
  const [columns, setColumns] = useState(1);
  useEffect(() => {
    const element = ref.current;
    if (element === null) return;
    const measure = () => {
      const width = element.clientWidth;
      setColumns(Math.max(1, Math.floor((width + GAP_X) / (min + GAP_X))));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [min]);
  return { ref, columns };
}

/** The second caption line: the provider and what people think of the image. */
export function sourceLine(
  candidate: ArtworkCandidate,
  names: Readonly<Record<string, string>>,
): string {
  if (candidate.provider === FRAMES_PROVIDER) return t("artworkPicker.source.frames");
  const provider = names[candidate.provider] ?? providerName(candidate.provider);
  const popularity = popularityOf(candidate);
  if (popularity === null) return provider;
  return popularity.kind === "likes"
    ? `${provider} · ${t("artwork.pop.likesShort", { count: popularity.count })}`
    : `${provider} · ${popularity.rating.toFixed(1)} · ${t("artwork.pop.votesShort", { count: popularity.count })}`;
}
