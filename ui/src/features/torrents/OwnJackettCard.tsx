import { ExternalLink, Eye, EyeOff, PowerOff } from "lucide-react";
import { useEffect, useState } from "react";

import { Button, IconButton } from "../../components/Button";
import { Field } from "../../components/Field";
import { StatusTag } from "../../components/StatusTag";
import { t } from "../../i18n/catalog";
import { ROW_BLOCK } from "../settings/FormParts";
import { useSettingsStore } from "../settings/settingsStore";
import { currentTorrentBackend } from "./torrentBackend";
import type { OwnJackett } from "./torrentBackend";

const URL_SETTING = "torrents.torznab.url";

type Outcome = { tone: "ok" | "problem"; text: string };

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** "Your Jackett is in use · 88 indexers". */
function inUseText(indexers: number | null): string {
  if (indexers === null) return t("torrents.jackett.inUse");
  return t("torrents.jackett.inUseCount", { count: indexers });
}

interface OwnJackettCardProps {
  found: OwnJackett;
  /** Whether CineVault's own indexer service is switched on. */
  managedOn: boolean;
  /** Switches CineVault's own indexer service off. */
  onManagedOff: () => void;
  /** Called after the own instance was saved, so the sources and the look-up read again. */
  onChanged: () => void;
}

/**
 * "Use your Jackett": shown when a Jackett the person runs answers on this computer (or at their
 * saved address). One button sets the own Torznab address to its all-indexers feed; the API key is
 * pasted (it is on the first page of Jackett's dashboard, which a button opens), tested, and kept
 * in the system credential store. While it is in use the card says how many indexers it searches
 * and offers to switch CineVault's own indexer service off, which is no longer needed.
 */
export function OwnJackettCard({ found, managedOn, onManagedOff, onChanged }: OwnJackettCardProps) {
  const [asking, setAsking] = useState(false);
  const [key, setKey] = useState("");
  const [reveal, setReveal] = useState(false);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [indexers, setIndexers] = useState<number | null>(null);

  // While in use, the saved key says how many indexers it has (one local request, no search).
  useEffect(() => {
    if (!found.inUse) return;
    let alive = true;
    void currentTorrentBackend()
      .testTorznab(found.feedUrl, null)
      .then((answer) => {
        if (!alive) return;
        if (answer.ok) setIndexers(answer.caps?.indexers ?? null);
        else setOutcome({ tone: "problem", text: answer.message });
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [found.inUse, found.feedUrl]);

  const openDashboard = () => {
    void currentTorrentBackend()
      .openPage(`${found.baseUrl}/UI/Dashboard`)
      .catch((error: unknown) => {
        setOutcome({ tone: "problem", text: messageOf(error) });
      });
  };

  const use = async () => {
    setBusy(true);
    setOutcome(null);
    try {
      const backend = currentTorrentBackend();
      const answer = await backend.testTorznab(found.feedUrl, key.trim());
      if (!answer.ok) {
        setOutcome({ tone: "problem", text: answer.message });
        return;
      }
      await backend.setTorznab(found.feedUrl, key.trim());
      useSettingsStore.getState().set(URL_SETTING, found.feedUrl);
      setIndexers(answer.caps?.indexers ?? null);
      setKey("");
      setReveal(false);
      setAsking(false);
      onChanged();
    } catch (error) {
      setOutcome({ tone: "problem", text: messageOf(error) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      aria-labelledby="torrent-jackett__title"
      data-testid="own-jackett"
      data-in-use={found.inUse}
      className={ROW_BLOCK}
    >
      <div>
        <div className="flex flex-wrap items-center gap-2">
          <h3 id="torrent-jackett__title" className="type-body m-0">
            {t("torrents.jackett.title")}
          </h3>
          {found.inUse ? (
            <StatusTag status="done">{t("torrents.jackett.inUseTag")}</StatusTag>
          ) : null}
        </div>
        <p className="mt-1 mb-0 max-w-[72ch] text-(length:--fs-12) leading-[1.5] text-text-muted">
          {found.inUse
            ? inUseText(indexers)
            : t("torrents.jackett.found", { address: found.baseUrl })}
        </p>
      </div>
      {!found.inUse && !asking ? (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="primary"
            onClick={() => {
              setAsking(true);
            }}
          >
            {t("torrents.jackett.use")}
          </Button>
        </div>
      ) : null}
      {!found.inUse && asking ? (
        <div className="flex flex-col gap-3">
          <div className="flex items-end gap-2">
            <Field
              className="flex-1"
              label={t("torrents.jackett.key")}
              description={t("torrents.jackett.keyHint")}
              type={reveal ? "text" : "password"}
              value={key}
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => {
                setKey(event.target.value);
                setOutcome(null);
              }}
            />
            <IconButton
              icon={reveal ? EyeOff : Eye}
              aria-label={reveal ? t("torrents.torznab.hideKey") : t("torrents.torznab.showKey")}
              aria-pressed={reveal}
              onClick={() => {
                setReveal(!reveal);
              }}
            />
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="primary"
              disabled={busy || key.trim() === ""}
              onClick={() => void use()}
            >
              {t("torrents.jackett.connect")}
            </Button>
            <Button icon={ExternalLink} onClick={openDashboard}>
              {t("torrents.jackett.openDashboard")}
            </Button>
            <Button
              variant="ghost"
              disabled={busy}
              onClick={() => {
                setAsking(false);
                setKey("");
                setOutcome(null);
              }}
            >
              {t("torrents.jackett.cancel")}
            </Button>
          </div>
        </div>
      ) : null}
      {found.inUse && managedOn ? (
        <div
          className="flex flex-wrap items-center gap-x-3 gap-y-2"
          data-testid="own-jackett-managed"
        >
          <span className="type-body text-text-2">{t("torrents.jackett.managedNote")}</span>
          <Button icon={PowerOff} onClick={onManagedOff}>
            {t("torrents.jackett.managedOff")}
          </Button>
        </div>
      ) : null}
      {outcome !== null ? (
        <p
          role="status"
          aria-live="polite"
          data-tone={outcome.tone}
          className={
            outcome.tone === "problem" ? "type-body m-0 text-danger" : "type-body m-0 text-text-2"
          }
        >
          {outcome.text}
        </p>
      ) : null}
    </section>
  );
}
