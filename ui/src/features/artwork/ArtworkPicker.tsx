import { useQueryClient } from "@tanstack/react-query";
import { ChevronDown } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";

import { Button } from "../../components/Button";
import { Menu, MenuRadioGroup, MenuRadioItem } from "../../components/Menu";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Skeleton } from "../../components/Skeleton";
import { useListSelection } from "../../components/useListSelection";
import { t } from "../../i18n/catalog";
import { activeLocale } from "../../i18n/locale";
import { cx } from "../../lib/cx";
import { ProviderNotes } from "../scrape/CandidateList";
import { ArtworkFooter } from "./ArtworkFooter";
import { ArtworkLightbox } from "./ArtworkLightbox";
import { ArtworkTile } from "./ArtworkGrid";
import { tileMinWidth, useGridColumns } from "./gridLayout";
import { ArtworkPreview } from "./ArtworkPreview";
import { ArtworkTypeStrip } from "./ArtworkTypeStrip";
import { applyChoice } from "./artworkApply";
import type { ArtworkChoice } from "./artworkApply";
import { choiceOfImage, releaseChoice } from "./artworkChoice";
import { dropAttributes, useFileDrop, useNativeFileDrop } from "./artworkDrop";
import { artworkLanguageLabel } from "./artworkLabels";
import type { ArtworkCandidate } from "./artworkBackend";
import {
  NO_TEXT,
  dimensions,
  neighboursOf,
  slotAspect,
  withFramesSettled,
  withoutLowQuality,
  FRAMES_PROVIDER,
} from "./artworkModel";
import {
  hasLanguage,
  languageChoices,
  providerLabel,
  sortForPicker,
  typeName,
} from "./pickerModel";
import type { PickerSort } from "./pickerModel";
import { useArtworkSlots } from "./useArtworkSlots";

/** What stepping through several titles adds to the picker. */
export interface BatchControls {
  /** "Use this and go to next" and Enter: the picker moves to the next title at once. */
  onNext: () => void;
  /** "Skip title": the title keeps its artwork and the picker moves on. */
  onSkip: () => void;
}

export interface ArtworkPickerProps {
  itemId: string;
  /** The title as the toasts name it: "Dune: Part Two (2024)". */
  title: string;
  /** The type the picker opens on. */
  initialSlot: string;
  /** Told when another type is chosen, so stepping to the next title keeps it. */
  onSlotChange?: (slot: string) => void;
  /** The page header above the picker. */
  header: ReactNode;
  /** Present when stepping through several titles. */
  batch?: BatchControls;
  /** An image was put in place. */
  onApplied?: (slot: string) => void;
  /** Cancel in the preview: closes the picker without writing. */
  onCancel: () => void;
  /** The only types offered (a collection has a poster and a fanart). */
  onlySlots?: readonly string[];
}

const LANGUAGE_LIMIT_REASON = 3;

/** The word on a language chip: "All", "English", "Textless". */
function languageLabel(value: string): string {
  if (value === "") return t("artworkPicker.language.all");
  if (value === NO_TEXT) return t("artworkPicker.language.textless");
  return artworkLanguageLabel(value, activeLocale());
}

/**
 * The artwork picker for one title: the type strip with its
 * counts, the language chips and sort menu, the grid of images with the current one pinned first,
 * "Use a file on this PC…" and "From a link…", and the large preview. A click only chooses and
 * shows the image in the preview; "Use this" or Enter writes it. A double click on an image, Space
 * on the chosen one, or the expand button on the preview opens it large (look only, nothing is
 * chosen). A picture dropped anywhere on the page joins the preview the same way. Nothing is
 * written until then.
 */
export function ArtworkPicker({
  itemId,
  title,
  initialSlot,
  onSlotChange,
  header,
  batch,
  onApplied,
  onCancel,
  onlySlots,
}: ArtworkPickerProps) {
  const queryClient = useQueryClient();
  const [slot, setSlotState] = useState(initialSlot);
  const search = useArtworkSlots(itemId, slot, onlySlots);
  const { states, names, slots } = search;
  const state = states[slot];

  const [language, setLanguage] = useState("");
  const [sort, setSort] = useState<PickerSort>("votes");
  const [showAll, setShowAll] = useState(false);
  /** The image put in place in this visit, shown as the current one before the lists are read again. */
  const [placed, setPlaced] = useState<Readonly<Record<string, ArtworkCandidate>>>({});
  /** A picture of the user's (file, link) chosen instead of a candidate. */
  const [other, setOther] = useState<ArtworkChoice | null>(null);
  const applying = useRef(false);
  /**
   * The picture open large: a candidate's id, or `null` for whatever the preview pane shows. Only
   * looks; it never changes what is chosen.
   */
  const [viewing, setViewing] = useState<{ id: string | null } | null>(null);
  const closeViewing = useCallback(() => setViewing(null), []);

  const items = useMemo(() => state?.items ?? [], [state]);
  const online = (state?.providers ?? []).some(
    (status) => status.provider !== FRAMES_PROVIDER && status.state === "searching",
  );
  const current = placed[slot] ?? state?.current ?? null;

  const chips = useMemo(() => languageChoices(items), [items]);
  const tiles = useMemo(() => {
    const settled = withFramesSettled(items, online, "");
    const filtered = settled.filter((candidate) => hasLanguage(candidate, language));
    const { shown } = withoutLowQuality(sortForPicker(filtered, sort), showAll);
    const rest = shown.filter((candidate) => candidate.id !== current?.id);
    return current === null ? rest : [current, ...rest];
  }, [items, online, language, sort, showAll, current]);
  const hiddenCount = useMemo(() => {
    const filtered = items.filter((candidate) => hasLanguage(candidate, language));
    return withoutLowQuality(filtered, false).hidden;
  }, [items, language]);

  const keys = useMemo(() => tiles.map((candidate) => candidate.id), [tiles]);
  const batchMode = batch !== undefined;
  // The narrower grid of the stepping picker fits two wide images across.
  const minTile =
    batchMode && slot !== "banner" ? Math.min(tileMinWidth(slot), 180) : tileMinWidth(slot);
  const { ref: gridRef, columns } = useGridColumns(minTile);

  const candidateChosen = (key: string | null): ArtworkChoice | null => {
    const found = tiles.find((candidate) => candidate.id === key);
    return found === undefined ? null : { kind: "candidate", candidate: found };
  };

  const commit = async (): Promise<boolean> => {
    const choice = other ?? candidateChosen(selection.current);
    if (choice === null || applying.current) return false;
    if (choice.kind === "candidate" && choice.candidate.id === current?.id) return false;
    applying.current = true;
    const before = placed[slot];
    // The picked image is the current one at once; a refusal puts the old one back.
    if (choice.kind === "candidate") {
      setPlaced((previous) => ({ ...previous, [slot]: choice.candidate }));
    }
    setOther(null);
    selection.clear();
    const ok = await applyChoice(queryClient, itemId, title, slot, choice);
    applying.current = false;
    releaseChoice(choice);
    if (!ok) {
      setPlaced((previous) => {
        const rest = Object.fromEntries(Object.entries(previous).filter(([key]) => key !== slot));
        return before === undefined ? rest : { ...rest, [slot]: before };
      });
      return false;
    }
    // A file or a link has no thumbnail of its own: its type is searched again for the new image.
    if (choice.kind !== "candidate") search.reload(slot);
    onApplied?.(slot);
    return true;
  };
  const activate = () => {
    if (batch === undefined) void commit();
    else {
      void commit();
      batch.onNext();
    }
  };
  const selection = useListSelection<string>({ keys, onOpen: activate, columns });

  const chosen: ArtworkChoice | null = other ?? candidateChosen(selection.current);
  const sameAsCurrent =
    chosen?.kind === "candidate" && current !== null && chosen.candidate.id === current.id;
  const usable = chosen !== null && !sameAsCurrent;
  const reason = usable
    ? undefined
    : chosen === null
      ? t("artworkPicker.use.none")
      : t("artworkPicker.use.same");
  const type = typeName(slot).toLowerCase();

  const changeSlot = (next: string) => {
    if (next === slot) return;
    releaseChoice(other);
    setOther(null);
    selection.clear();
    setLanguage("");
    setShowAll(false);
    setSlotState(next);
    onSlotChange?.(next);
  };
  const choose = (choice: ArtworkChoice) => {
    releaseChoice(other);
    selection.clear();
    setOther(choice);
  };

  // Pictures dropped on the page: a browser hands over the file, the desktop window its path.
  const drop = useFileDrop((images) => {
    const choice = images[0] === undefined ? null : choiceOfImage(images[0]);
    if (choice !== null) choose(choice);
  });
  useNativeFileDrop((target, images) => {
    if (target.itemId !== itemId) return;
    const choice = images[0] === undefined ? null : choiceOfImage(images[0]);
    if (choice !== null) choose(choice);
  });

  const waiting = (state === undefined || state.searching) && items.length === 0;
  const counts = Object.fromEntries(
    Object.entries(states).map(([key, value]) => [
      key,
      value.searching && value.items.length === 0 ? value.count : value.items.length,
    ]),
  );
  const currents = Object.fromEntries(
    Object.entries(states).map(([key, value]) => [key, placed[key] ?? value.current]),
  );
  // What the large view shows: the candidate it was opened on, else what the preview pane shows.
  const viewedCandidate =
    viewing === null
      ? null
      : viewing.id !== null
        ? (tiles.find((candidate) => candidate.id === viewing.id) ?? null)
        : chosen?.kind === "candidate"
          ? chosen.candidate
          : chosen === null
            ? current
            : null;
  const viewedUrl =
    viewing !== null && viewing.id === null && (chosen?.kind === "file" || chosen?.kind === "link")
      ? chosen.url
      : null;
  const stepViewing = useCallback(
    (delta: -1 | 1) => {
      const at = tiles.findIndex((candidate) => candidate.id === viewedCandidate?.id);
      if (at === -1 || tiles.length < 2) return;
      const next = tiles[(at + delta + tiles.length) % tiles.length];
      if (next !== undefined) setViewing({ id: next.id });
    },
    [tiles, viewedCandidate],
  );
  const useTip =
    current === null
      ? t("artworkPicker.use.tipNew", { type })
      : t("artworkPicker.use.tip", { type });

  return (
    <div className="flex h-full min-h-0 flex-col">
      {header}
      <div
        {...dropAttributes({ itemId, slot })}
        {...drop.handlers}
        data-dragging={drop.over ? "true" : undefined}
        className={cx(
          "group relative flex min-h-0 flex-1",
          "data-[art-over]:outline-2 data-[art-over]:-outline-offset-2 data-[art-over]:outline-gold",
          "data-[dragging=true]:outline-2 data-[dragging=true]:-outline-offset-2 data-[dragging=true]:outline-gold",
        )}
      >
        <ArtworkTypeStrip
          slots={slots}
          active={slot}
          onSelect={changeSlot}
          counts={counts}
          currents={currents}
          width={batchMode ? 180 : 196}
        />
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex h-[46px] shrink-0 items-center gap-2 border-b border-solid border-line-section px-6">
            <SegmentedControl<string>
              aria-label={t("artworkPicker.language")}
              value={language}
              onChange={(value) => {
                setLanguage(value);
                selection.clear();
              }}
              options={chips.slice(0, batchMode ? 3 : LANGUAGE_LIMIT_REASON + 1).map((chip) => ({
                value: chip.value,
                label: languageLabel(chip.value),
                count: chip.count,
              }))}
            />
            <span className="grow" />
            <Menu
              align="end"
              aria-label={t("artworkPicker.sort.label")}
              trigger={
                <Button variant="ghost" size="sm" trailingIcon={ChevronDown}>
                  {t("artworkPicker.sort", { by: t(`artworkPicker.sort.${sort}`) })}
                </Button>
              }
            >
              <MenuRadioGroup value={sort} onValueChange={(value) => setSort(value as PickerSort)}>
                <MenuRadioItem value="votes">{t("artworkPicker.sort.votes")}</MenuRadioItem>
                <MenuRadioItem value="size">{t("artworkPicker.sort.size")}</MenuRadioItem>
              </MenuRadioGroup>
            </Menu>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
            <div
              ref={gridRef}
              role="listbox"
              aria-label={t("artworkPicker.images")}
              aria-activedescendant={
                selection.current === null || !keys.includes(selection.current)
                  ? undefined
                  : `art-tile-${keys.indexOf(selection.current)}`
              }
              tabIndex={0}
              onKeyDown={(event) => {
                // Space looks at the chosen image large instead of toggling it: the picker
                // chooses one image, so there is nothing to toggle.
                if (
                  event.key === " " &&
                  selection.current !== null &&
                  keys.includes(selection.current)
                ) {
                  event.preventDefault();
                  setViewing({ id: selection.current });
                  return;
                }
                if (selection.onKeyDown(event)) {
                  event.preventDefault();
                  releaseChoice(other);
                  setOther(null);
                }
              }}
              style={{
                gridTemplateColumns: `repeat(auto-fill, minmax(${minTile}px, 1fr))`,
              }}
              className="grid gap-x-3.5 gap-y-[18px] outline-none"
            >
              {tiles.map((candidate, index) => {
                const isCurrent = candidate.id === current?.id;
                const pointer = selection.itemProps(candidate.id);
                return (
                  <ArtworkTile
                    key={candidate.id}
                    id={`art-tile-${index}`}
                    candidate={candidate}
                    slot={slot}
                    names={names}
                    current={isCurrent}
                    chosen={chosen?.kind === "candidate" && chosen.candidate.id === candidate.id}
                    compact={batchMode}
                    onClick={(event) => {
                      releaseChoice(other);
                      setOther(null);
                      pointer.onClick(event);
                    }}
                    onDoubleClick={() => setViewing({ id: candidate.id })}
                  />
                );
              })}
            </div>
            {waiting ? (
              <div aria-busy="true" className="grid grid-cols-4 gap-x-3.5 gap-y-[18px]">
                {[0, 1, 2, 3].map((n) => (
                  <Skeleton key={n} shape="tile" aspect="2 / 3" />
                ))}
              </div>
            ) : null}
            {!waiting && tiles.length === 0 ? (
              <p role="status" className="m-0 text-text-muted">
                {items.length === 0
                  ? t("artworkPicker.empty", { type })
                  : t("artworkPicker.emptyLanguage")}
              </p>
            ) : null}
            {hiddenCount > 0 && !showAll ? (
              <Button
                className="mt-4"
                title={t("artworkPicker.moreHidden.tip")}
                onClick={() => setShowAll(true)}
              >
                {t("artworkPicker.moreHidden", { count: hiddenCount })}
              </Button>
            ) : null}
            <ProviderNotes
              providers={state?.providers ?? []}
              names={names}
              onRetry={(provider) => search.retry(slot, provider)}
            />
          </div>
          <ArtworkFooter onChoice={choose} />
        </div>
        <ArtworkPreview
          slot={slot}
          chosen={chosen}
          current={current}
          names={names}
          width={batchMode ? "clamp(400px, 34vw, 520px)" : 420}
          onExpand={() => setViewing({ id: null })}
          {...(batchMode ? { heading: `${t("artworkPicker.preview")} · ${typeName(slot)}` } : {})}
          actions={
            batch === undefined ? (
              <>
                <Button
                  variant="primary"
                  size="lg"
                  className="flex-1"
                  title={useTip}
                  {...(reason === undefined ? {} : { disabledReason: reason })}
                  onClick={() => void commit()}
                >
                  {t("artworkPicker.use")}
                </Button>
                <Button size="lg" onClick={onCancel}>
                  {t("artworkPicker.cancel")}
                </Button>
              </>
            ) : (
              <>
                <Button
                  variant="primary"
                  size="lg"
                  className="flex-1"
                  shortcut="Enter"
                  title={useTip}
                  {...(reason === undefined ? {} : { disabledReason: reason })}
                  onClick={activate}
                >
                  {t("artworkPicker.useNext")}
                </Button>
                <Button
                  size="lg"
                  {...(reason === undefined ? {} : { disabledReason: reason })}
                  onClick={() => void commit()}
                >
                  {t("artworkPicker.use")}
                </Button>
                <Button
                  size="lg"
                  variant="ghost"
                  title={t("artworkPicker.skip.tip")}
                  onClick={batch.onSkip}
                >
                  {t("artworkPicker.skip")}
                </Button>
              </>
            )
          }
        />
        <div
          aria-hidden="true"
          className={cx(
            "pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-scrim p-6 text-center text-scrim-text",
            "opacity-0 transition-opacity duration-(--dur-hover) ease-out",
            "group-data-[art-over]:opacity-100 group-data-[dragging=true]:opacity-100",
          )}
        >
          {t("artworkPicker.drop", { type })}
        </div>
      </div>
      {viewing !== null && (viewedCandidate !== null || viewedUrl !== null) ? (
        <ArtworkLightbox
          image={
            viewedCandidate !== null
              ? {
                  kind: "candidate",
                  id: viewedCandidate.id,
                  image: viewedCandidate.image,
                  neighbours: neighboursOf(tiles, viewedCandidate.id),
                }
              : { kind: "url", url: viewedUrl ?? "" }
          }
          caption={(pixels) =>
            viewedCandidate !== null
              ? [
                  typeName(slot),
                  pixels !== null
                    ? `${pixels.width}×${pixels.height}`
                    : dimensions(viewedCandidate),
                  providerLabel(viewedCandidate, names),
                ]
                  .filter(Boolean)
                  .join(" · ")
              : [
                  typeName(slot),
                  chosen?.kind === "file" ? chosen.name : t("artworkPicker.source.link"),
                ].join(" · ")
          }
          aspect={slotAspect(slot)}
          onClose={closeViewing}
          {...(viewedCandidate === null ? {} : { onStep: stepViewing })}
        />
      ) : null}
    </div>
  );
}
