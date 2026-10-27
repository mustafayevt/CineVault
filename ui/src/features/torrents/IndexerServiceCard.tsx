import { ExternalLink, Download, RefreshCw } from "lucide-react";
import { useState } from "react";

import { Button } from "../../components/Button";
import { StatusTag } from "../../components/StatusTag";
import { Switch } from "../../components/Switch";
import { t } from "../../i18n/catalog";
import { CopyButton } from "../disclosure/CopyButton";
import { Field } from "../../components/Field";
import { ROW_BLOCK, FormRow } from "../settings/FormParts";
import { useSettingsStore } from "../settings/settingsStore";
import { textFormatError } from "../settings/textFormats";
import { checkedText, healthTag } from "./sourceHealth";
import { currentTorrentBackend } from "./torrentBackend";
import type { IndexerInfo, IndexerServiceStatus } from "./torrentBackend";
import type { useIndexerService } from "./useIndexerService";
import { TrackerLanguages } from "./TrackerLanguages";

const SOLVER_SETTING = "torrents.indexer-service.solver-url";

/** The challenge solver address: a web address, saved when it is one and shown when it is not. */
function SolverField({ saved }: { saved: string }) {
  const [draft, setDraft] = useState(saved);
  const problem = textFormatError("url", draft);
  return (
    <FormRow
      label={t("settings.torrents.indexer-service.solver-url.title")}
      hint={t("settings.torrents.indexer-service.solver-url.description")}
    >
      <Field
        mono
        aria-label={t("settings.torrents.indexer-service.solver-url.title")}
        placeholder="http://localhost:8191"
        autoComplete="off"
        spellCheck={false}
        value={draft}
        error={problem ?? undefined}
        onChange={(event) => {
          setDraft(event.target.value);
          if (textFormatError("url", event.target.value) === null) {
            useSettingsStore.getState().set(SOLVER_SETTING, event.target.value.trim());
          }
        }}
      />
    </FormRow>
  );
}
/** Where the free challenge solver CineVault can use is described and downloaded. */
const SOLVER_HOME = "https://github.com/FlareSolverr/FlareSolverr#installation";

/** The tag and word for a state of the service. */
function stateTag(status: IndexerServiceStatus | null): {
  tag: "done" | "waiting" | "missing" | "in-progress" | "off";
  label: string;
} {
  switch (status?.state) {
    case "running":
      return { tag: "done", label: t("torrents.indexer.state.running") };
    case "downloading":
      return { tag: "in-progress", label: t("torrents.indexer.state.downloading") };
    case "starting":
      return { tag: "in-progress", label: t("torrents.indexer.state.starting") };
    case "installed":
      return { tag: "off", label: t("torrents.indexer.state.installed") };
    case "stopped":
      return { tag: "missing", label: t("torrents.indexer.state.stopped") };
    case "failed":
      return { tag: "missing", label: t("torrents.indexer.state.failed") };
    case "notInstalled":
      return { tag: "off", label: t("torrents.indexer.state.notInstalled") };
    default:
      return { tag: "waiting", label: t("torrents.indexer.state.reading") };
  }
}

/** One line under the state: the version, what the last check found, or why it failed. */
function detailLine(status: IndexerServiceStatus | null): string {
  if (status === null) return "";
  if (status.state === "failed") return status.reason ?? t("torrents.indexer.failedUnknown");
  if (status.state === "downloading") return t("torrents.indexer.downloadingNote");
  if (status.state === "notInstalled") return t("torrents.indexer.notDownloaded");
  const parts: string[] = [];
  if (status.version !== null)
    parts.push(t("torrents.indexer.version", { version: status.version }));
  if (status.state === "running") {
    if (status.checking) parts.push(t("torrents.indexer.checking"));
    else if (status.working + status.failed > 0) {
      parts.push(
        status.working === 1
          ? t("torrents.indexer.workingOne")
          : t("torrents.indexer.working", { count: status.working }),
      );
      if (status.failed > 0) {
        parts.push(t("torrents.indexer.failedCount", { count: status.failed }));
      }
    }
  }
  return parts.join(" · ");
}

interface IndexerRowProps {
  indexer: IndexerInfo;
  now: number;
  busy: boolean;
  onSwitch: (on: boolean) => void;
}

function IndexerRow({ indexer, now, busy, onSwitch }: IndexerRowProps) {
  const tag = healthTag(indexer.state);
  const labelId = `torrent-indexer-${indexer.id}__name`;
  return (
    <li
      data-indexer={indexer.id}
      className="flex flex-wrap items-center gap-x-6 gap-y-2 border-t border-solid border-line-row px-4 py-3"
    >
      <div className="min-w-40 flex-1">
        <span id={labelId} className="type-body">
          {indexer.name}
        </span>
        {indexer.reason !== null && indexer.reason !== "" && !indexer.userOff ? (
          <p className="mt-1 mb-0 text-(length:--fs-12) leading-[1.5] text-text-muted">
            {indexer.reason}
          </p>
        ) : null}
      </div>
      <div className="ml-auto flex flex-wrap items-center justify-end gap-x-4 gap-y-2">
        <StatusTag status={tag.status}>{tag.label}</StatusTag>
        <span className="type-small min-w-28 text-text-muted">
          {indexer.userOff ? "" : checkedText(indexer.checkedAt, now)}
        </span>
        <Switch
          checked={!indexer.userOff}
          aria-labelledby={labelId}
          disabled={busy}
          onCheckedChange={onSwitch}
        />
      </div>
    </li>
  );
}

interface IndexerServiceCardProps {
  /** The saved challenge solver address ("" while there is none). */
  solverUrl: string;
  service: ReturnType<typeof useIndexerService>;
  /** The switch of the service: on means it is downloaded and runs while the app runs. */
  on: boolean;
  switching: boolean;
  onSwitch: (on: boolean) => void;
  now: number;
}

/**
 * The indexer service: its switch, what state it is in, Download and Update, the page of the service
 * with its admin password behind a Copy button, the challenge solver address and, under it, each
 * indexer with its health and switch.
 */
export function IndexerServiceCard({
  solverUrl,
  service,
  on,
  switching,
  onSwitch,
  now,
}: IndexerServiceCardProps) {
  const { status } = service;
  const [password, setPassword] = useState<string | null>(null);
  const state = stateTag(status);
  const canDownload = status?.state === "notInstalled" || status?.state === "failed";
  const running = status?.state === "running";
  const solverSet = solverUrl.trim() !== "";
  const solverWanted = service.indexers.filter(
    (indexer) => indexer.state === "needsSolver" && !indexer.userOff,
  ).length;
  return (
    <section
      id="torrents.indexer-service.enabled"
      aria-labelledby="torrent-indexer__title"
      data-testid="indexer-card"
      data-state={status?.state}
      className="flex flex-col"
    >
      <div className={ROW_BLOCK}>
        <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
          <div className="min-w-56 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <h3 id="torrent-indexer__title" className="type-body m-0">
                {t("torrents.indexer.title")}
              </h3>
              <StatusTag status={state.tag}>{state.label}</StatusTag>
            </div>
            <p className="mt-1 mb-0 max-w-[72ch] text-(length:--fs-12) leading-[1.5] text-text-muted">
              {t("settings.torrents.indexer-service.enabled.description")}
            </p>
            <p
              className="type-small mt-1 mb-0 text-text-2"
              role="status"
              aria-live="polite"
              data-testid="indexer-detail"
            >
              {on ? detailLine(status) : t("torrents.indexer.offNote")}
            </p>
          </div>
          <Switch
            checked={on}
            aria-labelledby="torrent-indexer__title"
            disabled={switching}
            onCheckedChange={onSwitch}
          />
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {canDownload ? (
            <Button
              icon={Download}
              disabled={service.working}
              onClick={() => {
                void service.download();
              }}
            >
              {status?.state === "failed"
                ? t("torrents.indexer.tryAgain")
                : t("torrents.indexer.download")}
            </Button>
          ) : null}
          {status?.latestVersion !== null && status?.latestVersion !== undefined ? (
            <Button
              icon={RefreshCw}
              disabled={service.working || status.state === "downloading"}
              onClick={() => {
                void service.update();
              }}
            >
              {t("torrents.indexer.update")}
            </Button>
          ) : null}
          {running ? (
            <Button
              icon={ExternalLink}
              disabled={service.working}
              onClick={() => {
                void service.open().then((access) => {
                  setPassword(access?.password ?? null);
                });
              }}
            >
              {t("torrents.indexer.open")}
            </Button>
          ) : null}
          {status?.latestVersion !== null && status?.latestVersion !== undefined ? (
            <span className="type-small text-text-muted" data-testid="indexer-latest">
              {t("torrents.indexer.latest", { version: status.latestVersion })}
            </span>
          ) : null}
        </div>
        {password !== null ? (
          <div
            className="flex flex-wrap items-center gap-2"
            role="group"
            aria-label={t("torrents.indexer.passwordGroup")}
            data-testid="indexer-password"
          >
            <span className="type-small text-text-2">{t("torrents.indexer.passwordNote")}</span>
            <span aria-hidden="true" className="type-code text-text-muted">
              ••••••••••••
            </span>
            <CopyButton
              value={password}
              label={t("torrents.indexer.copyPassword")}
              message={t("torrents.indexer.passwordCopied")}
            />
          </div>
        ) : null}
      </div>
      <SolverField saved={solverUrl} />
      {on && solverWanted > 0 && !solverSet ? (
        <div className={ROW_BLOCK} data-testid="solver-help">
          <p className="type-body m-0 max-w-[72ch] text-text-2">
            {t("torrents.indexer.solverHelp", { count: solverWanted })}
          </p>
          <div>
            <Button
              icon={ExternalLink}
              onClick={() => {
                void currentTorrentBackend()
                  .openPage(SOLVER_HOME)
                  .catch(() => undefined);
              }}
            >
              {t("torrents.indexer.solverLink")}
            </Button>
          </div>
        </div>
      ) : null}
      {on ? (
        <TrackerLanguages
          status={status}
          onChanged={() => {
            if (running) void service.check();
          }}
        />
      ) : null}
      {on && service.indexers.length > 0 ? (
        <ul
          aria-label={t("torrents.indexer.listLabel")}
          className="m-0 list-none p-0"
          data-testid="indexer-list"
        >
          {service.indexers.map((indexer) => (
            <IndexerRow
              key={indexer.id}
              indexer={indexer}
              now={now}
              busy={service.working}
              onSwitch={(next) => {
                void service.setIndexer(indexer.id, next);
              }}
            />
          ))}
        </ul>
      ) : null}
    </section>
  );
}
