import { useQuery } from "@tanstack/react-query";
import { useId, useState } from "react";
import type { ReactNode } from "react";

import { Button } from "../../components/Button";
import { Checkbox } from "../../components/Checkbox";
import { Chip } from "../../components/Chip";
import { Field } from "../../components/Field";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Select } from "../../components/Select";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { useEscapeLayer } from "../shell/escStack";
import { currentTorrentBackend } from "./torrentBackend";
import type { WishCriteria, WishScope } from "./torrentBackend";
import type { TitleKind } from "./torrentsSearch";
import {
  RESOLUTIONS,
  SOURCES,
  draftOf,
  draftProblem,
  draftValue,
  sizeUnitBytes,
  sourceLabel,
} from "./wishlistModel";
import type { ResolutionChoice, ScopeChoice, WishDraft } from "./wishlistModel";

interface WishEditorProps {
  kind: TitleKind;
  tmdbId: number;
  scope: WishScope;
  criteria: WishCriteria;
  /** `add` shows "Add to Wishlist", `edit` shows "Save". */
  mode: "add" | "edit";
  saving: boolean;
  onSave: (scope: WishScope, criteria: WishCriteria) => void;
  onCancel: () => void;
}

/** One labelled row of the editor: the label in a fixed column, the control after it. */
function Row({ label, id, children }: { label: string; id: string; children: ReactNode }) {
  return (
    <div className="flex min-w-0 items-center gap-3 @max-[640px]/pane:flex-col @max-[640px]/pane:items-start @max-[640px]/pane:gap-1.5">
      <span
        id={id}
        className="w-[120px] shrink-0 text-(length:--fs-12) font-(weight:--fw-500) text-text-2"
      >
        {label}
      </span>
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">{children}</div>
    </div>
  );
}

/** The seasons and episodes of a show for the scope pickers; empty while they load. */
function useSeasonOptions(kind: TitleKind, tmdbId: number, season: number, wanted: boolean) {
  const seasons = useQuery({
    enabled: kind === "show" && wanted,
    queryKey: ["torrents", "seasons", tmdbId],
    queryFn: () => currentTorrentBackend().seasons(tmdbId),
    retry: false,
    staleTime: 10 * 60_000,
  });
  const episodes = useQuery({
    enabled: kind === "show" && wanted,
    queryKey: ["torrents", "episodes", tmdbId, season],
    queryFn: () => currentTorrentBackend().episodes(tmdbId, season),
    retry: false,
    staleTime: 10 * 60_000,
  });
  return { seasons: seasons.data ?? [], episodes: episodes.data ?? [] };
}

/**
 * The Wishlist editor (spec `torrent-wishlist` "Wishlist entries and criteria"), part of the page
 * rather than a dialog: for a show what to look for (the next new episode, a season or an
 * episode), then the lowest resolution, the sources, HDR only, the largest size and the fewest
 * seeders, with "Add to Wishlist" or "Save" and "Cancel".
 */
export function WishEditor({
  kind,
  tmdbId,
  scope,
  criteria,
  mode,
  saving,
  onSave,
  onCancel,
}: WishEditorProps) {
  const format = useFormat();
  const unit = sizeUnitBytes(format.sizeBase);
  const [draft, setDraft] = useState<WishDraft>(() => draftOf(scope, criteria, unit));
  const [tried, setTried] = useState(false);
  const ids = useId();
  const change = (patch: Partial<WishDraft>) => setDraft((current) => ({ ...current, ...patch }));
  const options = useSeasonOptions(
    kind,
    tmdbId,
    draft.season,
    draft.scope === "season" || draft.scope === "episode",
  );
  // Esc cancels, like it closes a picker page.
  useEscapeLayer("picker", true, onCancel);
  const problem = draftProblem(draft);
  const seasonOptions =
    options.seasons.length === 0
      ? [
          {
            value: String(draft.season),
            label: t("wishlist.scope.season", { season: draft.season }),
          },
        ]
      : options.seasons.map((season) => ({
          value: String(season.number),
          label:
            season.name.trim() === ""
              ? t("wishlist.scope.season", { season: season.number })
              : season.name,
        }));
  const episodeOptions =
    options.episodes.length === 0
      ? [
          {
            value: String(draft.episode),
            label: t("wishlist.edit.episode", { episode: draft.episode }),
          },
        ]
      : options.episodes.map((episode) => ({
          value: String(episode.number),
          label:
            episode.name.trim() === ""
              ? t("wishlist.edit.episode", { episode: episode.number })
              : `${episode.number}. ${episode.name}`,
        }));

  const save = () => {
    setTried(true);
    if (problem !== null) return;
    const value = draftValue(draft, unit);
    onSave(value.scope, value.criteria);
  };

  return (
    <form
      aria-label={t(mode === "add" ? "wishlist.edit.addRegion" : "wishlist.edit.editRegion")}
      data-testid="wish-editor"
      className="flex flex-col gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        save();
      }}
    >
      {kind === "show" ? (
        <Row label={t("wishlist.edit.lookFor")} id={`${ids}-scope`}>
          <SegmentedControl<ScopeChoice>
            aria-label={t("wishlist.edit.lookFor")}
            options={[
              { value: "next", label: t("wishlist.scope.next") },
              { value: "season", label: t("wishlist.edit.aSeason") },
              { value: "episode", label: t("wishlist.edit.anEpisode") },
            ]}
            value={draft.scope === "movie" ? "next" : draft.scope}
            onChange={(value) => change({ scope: value })}
          />
          {draft.scope === "season" || draft.scope === "episode" ? (
            <Select
              aria-label={t("wishlist.edit.season")}
              className="w-[180px]"
              options={seasonOptions}
              value={String(draft.season)}
              onValueChange={(value) => change({ season: Number(value), episode: 1 })}
            />
          ) : null}
          {draft.scope === "episode" ? (
            <Select
              aria-label={t("wishlist.edit.episodeLabel")}
              className="w-[220px]"
              options={episodeOptions}
              value={String(draft.episode)}
              onValueChange={(value) => change({ episode: Number(value) })}
            />
          ) : null}
        </Row>
      ) : null}
      <Row label={t("wishlist.edit.resolution")} id={`${ids}-resolution`}>
        <SegmentedControl<ResolutionChoice>
          aria-label={t("wishlist.edit.resolution")}
          options={RESOLUTIONS.map((value) => ({
            value,
            label: value === "any" ? t("wishlist.edit.any") : value,
          }))}
          value={draft.resolution}
          onChange={(value) => change({ resolution: value })}
        />
        {draft.resolution === "any" ? null : (
          <span className="text-(length:--fs-12) text-text-muted">
            {t("wishlist.edit.orBetter")}
          </span>
        )}
      </Row>
      <Row label={t("wishlist.edit.source")} id={`${ids}-source`}>
        <div
          role="group"
          aria-labelledby={`${ids}-source`}
          className="flex flex-wrap items-center gap-1.5"
        >
          {SOURCES.map((source) => {
            const on = draft.sources.includes(source);
            return (
              <Chip
                key={source}
                active={on}
                aria-pressed={on}
                onClick={() =>
                  change({
                    sources: on
                      ? draft.sources.filter((chosen) => chosen !== source)
                      : SOURCES.filter((s) => s === source || draft.sources.includes(s)),
                  })
                }
              >
                {sourceLabel(source)}
              </Chip>
            );
          })}
        </div>
        {draft.sources.length === 0 ? (
          <span className="text-(length:--fs-12) text-text-muted">
            {t("wishlist.edit.anySource")}
          </span>
        ) : null}
      </Row>
      <Row label={t("wishlist.edit.hdrLabel")} id={`${ids}-hdr`}>
        <Checkbox checked={draft.hdr} onCheckedChange={(hdr) => change({ hdr })}>
          {t("wishlist.edit.hdr")}
        </Checkbox>
      </Row>
      <Row label={t("wishlist.edit.limits")} id={`${ids}-limits`}>
        <span className="text-(length:--fs-12) text-text-2">{t("wishlist.edit.upTo")}</span>
        <Field
          aria-label={t("wishlist.edit.maxSize")}
          inputMode="decimal"
          placeholder={t("wishlist.edit.noLimit")}
          value={draft.maxSizeGb}
          onChange={(event) => change({ maxSizeGb: event.target.value })}
          boxClassName="w-[120px]"
          trailing={
            <span className="text-(length:--fs-12) text-text-muted">
              {format.sizeBase === "base2" ? "GiB" : "GB"}
            </span>
          }
        />
        <span className="ml-2 text-(length:--fs-12) text-text-2">{t("wishlist.edit.atLeast")}</span>
        <Field
          aria-label={t("wishlist.edit.minSeeders")}
          inputMode="numeric"
          placeholder={t("wishlist.edit.anySeeders")}
          value={draft.minSeeders}
          onChange={(event) => change({ minSeeders: event.target.value })}
          boxClassName="w-[120px]"
          trailing={
            <span className="text-(length:--fs-12) text-text-muted">
              {t("wishlist.edit.seeders")}
            </span>
          }
        />
      </Row>
      {tried && problem !== null ? (
        <p role="alert" className="m-0 text-(length:--fs-12) text-danger">
          {problem}
        </p>
      ) : null}
      <div className="flex items-center gap-2 pl-[132px] @max-[640px]/pane:pl-0">
        <Button type="submit" variant="primary" disabled={saving}>
          {t(mode === "add" ? "wishlist.add" : "wishlist.edit.save")}
        </Button>
        <Button type="button" variant="ghost" onClick={onCancel}>
          {t("wishlist.edit.cancel")}
        </Button>
      </div>
    </form>
  );
}
