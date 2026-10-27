import { RefreshCw } from "lucide-react";

import { Button } from "../../components/Button";
import { Switch } from "../../components/Switch";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import { useNow } from "../drives/useNow";
import { IndexerServiceCard } from "../torrents/IndexerServiceCard";
import { OwnJackettCard } from "../torrents/OwnJackettCard";
import { TorznabCard } from "../torrents/TorznabCard";
import { checkedText, healthTag } from "../torrents/sourceHealth";
import type { SourceStatus } from "../torrents/torrentBackend";
import { useIndexerService } from "../torrents/useIndexerService";
import { useOwnJackett } from "../torrents/useOwnJackett";
import { useSourceStatuses } from "../torrents/useSourceStatuses";
import { FormRow, SettingsSection } from "./FormParts";
import { useSettingsStore } from "./settingsStore";

/** The sites Torrent search asks that CineVault knows by itself. */
const BUILT_IN = [
  ["torrentio", "Torrentio"],
  ["yts", "YTS"],
  ["eztv", "EZTV"],
  ["apibay", "The Pirate Bay"],
  ["knaben", "Knaben"],
  ["nyaa", "Nyaa"],
] as const;

/** The tag colour for the answer a site gave: working is green, trouble is orange. */
function variantOf(status: string): "ok" | "warn" | "neutral" {
  if (status === "done") return "ok";
  return status === "needs-key" || status === "missing" ? "warn" : "neutral";
}

/** The switch settings by site; a site that has none stored is on. */
function storedOn(values: Readonly<Record<string, unknown>>, id: string): boolean {
  return values[`torrents.source.${id}`] !== false;
}

interface SourceRowProps {
  source: SourceStatus;
  now: number;
  busy: boolean;
  onSwitch: (on: boolean) => void;
}

function SourceRow({ source, now, busy, onSwitch }: SourceRowProps) {
  const tag = healthTag(source.on ? source.health : "off");
  const labelId = `torrent-source-${source.id}__name`;
  return (
    <div data-source={source.id} role="listitem">
      <FormRow
        labelId={labelId}
        label={source.name}
        hint={
          source.on && source.reason !== null && source.reason !== "" ? source.reason : undefined
        }
      >
        <div className="flex items-center gap-3">
          <span data-status={tag.status}>
            <Tag variant={variantOf(tag.status)}>{tag.label}</Tag>
          </span>
          <span className="min-w-28 text-(length:--fs-12) text-text-muted">
            {source.on ? checkedText(source.checkedAt, now) : ""}
          </span>
          <span className="flex-1" />
          <Switch
            checked={source.on}
            aria-labelledby={labelId}
            disabled={busy}
            onCheckedChange={onSwitch}
          />
        </div>
      </FormRow>
    </div>
  );
}

/**
 * Settings, Torrent sources (spec `app-shell` "Settings page"): "Use your Jackett" when one of
 * the person's own answers; the sites Torrent search asks, each with its switch, its health in
 * words and when it was last asked; the indexer service with its tracker languages and indexers;
 * and the person's own Torznab instance. Laid out like the other sections: rows of one card.
 */
export function TorrentSourcesSection() {
  const values = useSettingsStore((state) => state.values);
  const statuses = useSourceStatuses();
  const now = useNow(30_000);
  const indexer = useIndexerService();
  const jackett = useOwnJackett();
  const indexerOn = values["torrents.indexer-service.enabled"] !== false;
  // Until the first answer the list shows the sites from their switches, not yet checked.
  const builtIn: readonly SourceStatus[] =
    statuses.data === null
      ? BUILT_IN.map(([id, name]) => ({
          id,
          name,
          kind: "built-in",
          on: storedOn(values, id),
          health: "untested",
          reason: null,
          checkedAt: null,
        }))
      : statuses.data.filter((source) => source.kind === "built-in");
  const solver = values["torrents.indexer-service.solver-url"];
  const torznab = values["torrents.torznab.url"];

  return (
    <SettingsSection id="torrents" title={t("settingsPage.torrents.title")}>
      {jackett.found ? (
        <OwnJackettCard
          found={jackett.found}
          managedOn={indexerOn}
          onManagedOff={() => {
            void statuses.setSource("indexer-service", false).then(() => indexer.refresh());
          }}
          onChanged={() => {
            void statuses.refetch();
            void jackett.refresh();
          }}
        />
      ) : null}
      <FormRow label={t("torrents.sources.heading")} hint={t("settingsPage.torrents.sources.hint")}>
        <Button
          icon={RefreshCw}
          disabled={statuses.fetching}
          onClick={() => {
            // Checks the indexers too when the service is running, then reads everything again.
            const check = indexer.status?.state === "running" ? indexer.check() : Promise.resolve();
            void check.then(() => statuses.refetch());
          }}
        >
          {t("torrents.sources.check")}
        </Button>
      </FormRow>
      <div role="list" aria-label={t("torrents.sources.heading")}>
        {builtIn.map((source) => (
          <SourceRow
            key={source.id}
            source={source}
            now={now}
            busy={statuses.saving}
            onSwitch={(on) => {
              void statuses.setSource(source.id, on);
            }}
          />
        ))}
      </div>
      <IndexerServiceCard
        solverUrl={typeof solver === "string" ? solver : ""}
        service={indexer}
        on={indexerOn}
        switching={statuses.saving}
        now={now}
        onSwitch={(on) => {
          void statuses.setSource("indexer-service", on).then(() => indexer.refresh());
        }}
      />
      <TorznabCard
        savedUrl={typeof torznab === "string" ? torznab : ""}
        keySaved={statuses.data?.some((source) => source.id === "torznab" && source.on) ?? false}
        onChanged={() => {
          void statuses.refetch();
          void jackett.refresh();
        }}
      />
    </SettingsSection>
  );
}
