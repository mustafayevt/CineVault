import { Eye, EyeOff } from "lucide-react";
import { useState } from "react";

import { Button, IconButton } from "../../components/Button";
import { Field } from "../../components/Field";
import { t } from "../../i18n/catalog";
import { ROW_BLOCK } from "../settings/FormParts";
import { useSettingsStore } from "../settings/settingsStore";
import { currentTorrentBackend } from "./torrentBackend";

const URL_SETTING = "torrents.torznab.url";

interface TorznabCardProps {
  /** The saved address ("" while there is none). */
  savedUrl: string;
  /** Whether a key is saved for it. The key itself is never read back. */
  keySaved: boolean;
  /** Called after the instance was saved or removed, so the sources list reads again. */
  onChanged: () => void;
}

type Outcome = { tone: "ok" | "problem"; text: string };

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Your own Torznab instance: its address and key, a connection test with a plain answer, Save and
 * Remove. A saved key is kept in the system credential store and is never shown again; typing a new
 * one replaces it, leaving the field empty keeps it.
 */
export function TorznabCard({ savedUrl, keySaved, onChanged }: TorznabCardProps) {
  const [url, setUrl] = useState(savedUrl);
  const [key, setKey] = useState("");
  const [reveal, setReveal] = useState(false);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const hasInstance = savedUrl.trim() !== "";
  const address = url.trim();
  // An empty field keeps the saved key; typed text replaces it.
  const typedKey = key === "" ? null : key;

  const run = async (work: () => Promise<Outcome | null>) => {
    setBusy(true);
    setOutcome(null);
    try {
      setOutcome(await work());
    } catch (error) {
      setOutcome({ tone: "problem", text: messageOf(error) });
    } finally {
      setBusy(false);
    }
  };

  const test = () =>
    run(async () => {
      const answer = await currentTorrentBackend().testTorznab(address, typedKey);
      return { tone: answer.ok ? "ok" : "problem", text: answer.message };
    });

  const save = () =>
    run(async () => {
      await currentTorrentBackend().setTorznab(address, typedKey);
      useSettingsStore.getState().set(URL_SETTING, address);
      setKey("");
      setReveal(false);
      onChanged();
      return { tone: "ok", text: t("torrents.torznab.saved") };
    });

  const remove = () =>
    run(async () => {
      await currentTorrentBackend().setTorznab("", null);
      useSettingsStore.getState().set(URL_SETTING, "");
      setUrl("");
      setKey("");
      setReveal(false);
      onChanged();
      return { tone: "ok", text: t("torrents.torznab.removed") };
    });

  return (
    <section
      id={URL_SETTING}
      aria-labelledby="torrent-torznab__title"
      data-testid="torznab-card"
      className={ROW_BLOCK}
    >
      <div>
        <h3 id="torrent-torznab__title" className="type-body m-0">
          {t("settings.torrents.torznab.url.title")}
        </h3>
        <p className="mt-1 mb-0 max-w-[72ch] text-(length:--fs-12) leading-[1.5] text-text-muted">
          {t("settings.torrents.torznab.url.description")}
        </p>
      </div>
      <div className="flex flex-col gap-3">
        <Field
          label={t("torrents.torznab.address")}
          mono
          value={url}
          placeholder="http://localhost:9117/api/v2.0/indexers/all/results/torznab/api"
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => {
            setUrl(event.target.value);
            setOutcome(null);
          }}
        />
        <div className="flex items-end gap-2">
          <Field
            className="flex-1"
            label={t("torrents.torznab.key")}
            type={reveal ? "text" : "password"}
            value={key}
            placeholder={keySaved ? t("torrents.torznab.keySaved") : ""}
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
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <Button disabled={busy || address === ""} onClick={() => void test()}>
          {t("torrents.torznab.test")}
        </Button>
        <Button variant="primary" disabled={busy || address === ""} onClick={() => void save()}>
          {t("torrents.torznab.save")}
        </Button>
        {hasInstance ? (
          <Button variant="ghost" disabled={busy} onClick={() => void remove()}>
            {t("torrents.torznab.remove")}
          </Button>
        ) : null}
      </div>
      <p
        role="status"
        aria-live="polite"
        data-tone={outcome?.tone}
        className={
          outcome?.tone === "problem"
            ? "type-body m-0 min-h-5 text-danger"
            : "type-body m-0 min-h-5 text-text-2"
        }
      >
        {outcome?.text ?? ""}
      </p>
    </section>
  );
}
