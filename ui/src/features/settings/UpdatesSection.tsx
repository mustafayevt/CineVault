import { useRouter } from "@tanstack/react-router";

import { Button } from "../../components/Button";
import { Switch } from "../../components/Switch";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { lastCheckText } from "../updates/lastCheck";
import { ReleaseNotes } from "../updates/ReleaseNotes";
import { useUpdateStore } from "../updates/updateStore";
import { useAppVersion } from "../updates/useAppVersion";
import { useNow } from "../drives/useNow";
import { FormRow, SettingsSection } from "./FormParts";
import { useSettingsStore } from "./settingsStore";

/** The setting behind "Install updates when I close the app". Off until the user turns it on. */
const INSTALL_ON_CLOSE_KEY = "updates.install-on-close";

/** The line under "CineVault 0.2.5": what the last check found, or what the update is doing. */
function useStatusLine(): string {
  const format = useFormat();
  const now = useNow();
  const {
    phase,
    offer,
    progress,
    checking,
    checkResult,
    lastCheckedAt,
    jobsMessage,
    reason,
    error,
  } = useUpdateStore();
  if (error !== null) return error;
  const version = offer?.version ?? "";
  switch (phase) {
    case "downloading": {
      const share =
        progress !== null && progress.total > 0
          ? Math.min(100, Math.floor((progress.downloaded / progress.total) * 100))
          : 0;
      return t("settingsPage.updates.downloading", { version, percent: share });
    }
    case "installing":
      return t("updates.installing.title", { version });
    case "started":
      return reason ?? t("updates.started.body");
    case "jobs":
      return `${jobsMessage ?? t("updates.jobs.title")} ${t("updates.jobs.body")}`;
    case "waiting":
      return t("updates.waiting.title", { version });
    default:
      break;
  }
  if (checking) return t("settingsPage.updates.checking");
  if (checkResult?.tone === "danger") return checkResult.message;
  if (offer !== null && (phase === "available" || phase === "reviewing")) {
    return lastCheckText(lastCheckedAt, now, format.locale, t("updates.title", { version }));
  }
  return lastCheckText(lastCheckedAt, now, format.locale);
}

/** The buttons that fit the phase of the update. */
function UpdateButtons() {
  const store = useUpdateStore();
  const { phase, checking } = store;
  const busy = phase === "downloading" || phase === "installing";
  return (
    <>
      <Button
        disabled={checking || busy || phase === "waiting"}
        onClick={() => void store.checkNow()}
      >
        {t("settingsPage.updates.checkNow")}
      </Button>
      {phase === "available" || phase === "reviewing" ? (
        <Button variant="primary" onClick={() => void store.install()}>
          {t("updates.install")}
        </Button>
      ) : null}
      {phase === "downloading" ? (
        <Button onClick={() => void store.cancelDownload()}>{t("updates.cancel")}</Button>
      ) : null}
      {phase === "jobs" ? (
        <>
          <Button variant="primary" onClick={store.waitForJobs}>
            {t("updates.jobs.wait")}
          </Button>
          <Button onClick={() => void store.install(true)}>{t("updates.jobs.stop")}</Button>
          <Button variant="ghost" onClick={store.later}>
            {t("updates.notNow")}
          </Button>
        </>
      ) : null}
      {phase === "waiting" ? (
        <Button onClick={store.stopWaiting}>{t("updates.cancel")}</Button>
      ) : null}
    </>
  );
}

/**
 * Settings, Updates (spec `app-shell` "Version and update check in Settings" and "Install updates
 * when the app closes"): the version, the result of the last check, Check now and the switch,
 * and "What's new in X" with the release notes while a newer version is offered.
 */
export function UpdatesSection() {
  const version = useAppVersion();
  const status = useStatusLine();
  const installOnClose = useSettingsStore((state) => state.values[INSTALL_ON_CLOSE_KEY] === true);
  // The notes of the version that is offered, until it is installed or the offer goes.
  const offer = useUpdateStore((state) => state.offer);
  // Without a router (a section drawn on its own) the link has nowhere to go and is left out.
  const router = useRouter({ warn: false }) as ReturnType<typeof useRouter> | undefined;
  return (
    <SettingsSection id="updates" title={t("settingsPage.updates.title")}>
      <FormRow
        label={version === "" ? "CineVault" : t("settingsPage.updates.version", { version })}
        hint={
          <span role="status" aria-live="polite">
            {status}
          </span>
        }
      >
        <div className="flex flex-wrap items-center gap-x-2.5 gap-y-2">
          <UpdateButtons />
          <span className="flex-1" />
          <span id="install-on-close" className="text-text-2">
            {t("settingsPage.updates.installOnClose")}
          </span>
          <Switch
            checked={installOnClose}
            aria-labelledby="install-on-close"
            onCheckedChange={(on) => {
              useSettingsStore.getState().set(INSTALL_ON_CLOSE_KEY, on);
            }}
          />
        </div>
      </FormRow>
      {offer === null ? null : <ReleaseNotes offer={offer} />}
      {router === undefined ? null : (
        <FormRow
          label={t("settingsPage.updates.releaseNotes")}
          hint={t("settingsPage.updates.releaseNotes.hint")}
        >
          <Button
            onClick={() => {
              void router.navigate({ to: "/release-notes" });
            }}
          >
            {t("settingsPage.updates.releaseNotes.open")}
          </Button>
        </FormRow>
      )}
    </SettingsSection>
  );
}
