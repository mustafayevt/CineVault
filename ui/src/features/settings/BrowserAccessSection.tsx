import { Check, Copy } from "lucide-react";
import { useCallback, useEffect, useId, useState } from "react";

import { Button } from "../../components/Button";
import { Field } from "../../components/Field";
import { Switch } from "../../components/Switch";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import {
  BrowserAccessRequestError,
  browserLabel,
  currentBrowserAccessBackend,
} from "./browserAccess";
import type { BrowserAccess, BrowserSession } from "./browserAccess";
import { FormRow, ROW_BLOCK, SettingsSection } from "./FormParts";
import { QrCode } from "./QrCode";

/** The words beside the switch for each state the desktop reports. */
function stateText(state: string): string {
  switch (state) {
    case "running":
      return t("settingsPage.browserAccess.state.running");
    case "portInUse":
      return t("settingsPage.browserAccess.state.portInUse");
    case "error":
      return t("settingsPage.browserAccess.state.error");
    case "noPassword":
      return t("settingsPage.browserAccess.state.noPassword");
    default:
      return t("settingsPage.browserAccess.state.off");
  }
}

/** How often the section reads the state again while it shows (new browsers, last use). */
const REFRESH_MS = 5000;

function messageOf(error: unknown): string {
  if (error instanceof BrowserAccessRequestError || error instanceof Error) return error.message;
  return String(error);
}

/** Copies an address and says so on the button for two seconds. */
function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return undefined;
    const timer = window.setTimeout(() => setCopied(false), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);
  return (
    <Button
      size="sm"
      icon={copied ? Check : Copy}
      aria-label={t("settingsPage.browserAccess.copyAddress", { address: text })}
      onClick={() => {
        void navigator.clipboard
          ?.writeText(text)
          .then(() => setCopied(true))
          .catch(() => setCopied(false));
      }}
    >
      {copied ? t("settingsPage.browserAccess.copied") : t("settingsPage.browserAccess.copy")}
    </Button>
  );
}

/** The password fields: create when there is none, change behind a button otherwise. */
function PasswordRow({
  access,
  run,
}: {
  access: BrowserAccess;
  run: (work: () => Promise<BrowserAccess>, done?: string) => Promise<boolean>;
}) {
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const editing = !access.hasPassword || open;
  const save = async () => {
    const backend = currentBrowserAccessBackend();
    const ok = await run(
      () => backend.setPassword(password, confirm),
      access.hasPassword
        ? t("settingsPage.browserAccess.password.changed")
        : t("settingsPage.browserAccess.password.created"),
    );
    if (ok) {
      setOpen(false);
      setPassword("");
      setConfirm("");
    }
  };
  return (
    <FormRow
      label={t("settingsPage.browserAccess.password")}
      hint={
        access.hasPassword
          ? t("settingsPage.browserAccess.password.hint")
          : t("settingsPage.browserAccess.password.needed")
      }
      align="start"
    >
      {editing ? (
        <form
          className="flex max-w-[360px] flex-col gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <Field
            type="password"
            autoComplete="new-password"
            aria-label={t("settingsPage.browserAccess.password.new")}
            placeholder={t("settingsPage.browserAccess.password.new")}
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
          <Field
            type="password"
            autoComplete="new-password"
            aria-label={t("settingsPage.browserAccess.password.confirm")}
            placeholder={t("settingsPage.browserAccess.password.confirm")}
            value={confirm}
            onChange={(event) => setConfirm(event.target.value)}
          />
          <div className="flex gap-2">
            <Button type="submit" variant={access.hasPassword ? "default" : "primary"}>
              {access.hasPassword
                ? t("settingsPage.browserAccess.password.change")
                : t("settingsPage.browserAccess.password.create")}
            </Button>
            {access.hasPassword ? (
              <Button variant="ghost" onClick={() => setOpen(false)}>
                {t("settingsPage.browserAccess.cancel")}
              </Button>
            ) : null}
          </div>
        </form>
      ) : (
        <Button onClick={() => setOpen(true)}>
          {t("settingsPage.browserAccess.password.change")}
        </Button>
      )}
    </FormRow>
  );
}

/** The port field and its Apply button; the server restarts on the new port. */
function PortRow({
  access,
  run,
}: {
  access: BrowserAccess;
  run: (work: () => Promise<BrowserAccess>, done?: string) => Promise<boolean>;
}) {
  const [draft, setDraft] = useState(String(access.port));
  const [error, setError] = useState<string | undefined>(undefined);
  const apply = async () => {
    const port = Number(draft);
    if (!Number.isInteger(port) || port < 1024 || port > 65535) {
      setError(t("settingsPage.browserAccess.port.invalid"));
      return;
    }
    setError(undefined);
    await run(() => currentBrowserAccessBackend().setPort(port));
  };
  return (
    <FormRow
      label={t("settingsPage.browserAccess.port")}
      hint={t("settingsPage.browserAccess.port.hint")}
      align="start"
    >
      <form
        className="flex items-start gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          void apply();
        }}
      >
        <Field
          inputMode="numeric"
          aria-label={t("settingsPage.browserAccess.port")}
          className="w-[96px]"
          mono
          value={draft}
          error={error}
          onChange={(event) => setDraft(event.target.value.replace(/[^0-9]/g, ""))}
        />
        <Button type="submit" disabled={draft === String(access.port)}>
          {t("settingsPage.browserAccess.port.apply")}
        </Button>
      </form>
    </FormRow>
  );
}

/** One signed-in browser: what it is, where from, when it was last used, and Sign out. */
function SessionLine({ session, onSignOut }: { session: BrowserSession; onSignOut: () => void }) {
  const format = useFormat();
  const label = browserLabel(session.agent);
  const name =
    label === null
      ? t("settingsPage.browserAccess.sessions.unknown")
      : t("settingsPage.browserAccess.sessions.browser", label);
  return (
    <li className="flex items-center justify-between gap-3 py-1">
      <div className="min-w-0">
        <div className="truncate">{name}</div>
        <div className="text-(length:--fs-12) text-text-muted">
          {t("settingsPage.browserAccess.sessions.detail", {
            address: session.address,
            when: session.lastUsedMs === null ? "" : format.dateTime(session.lastUsedMs),
          })}
        </div>
      </div>
      <Button
        size="sm"
        onClick={onSignOut}
        aria-label={t("settingsPage.browserAccess.sessions.signOutOne", { name })}
      >
        {t("settingsPage.browserAccess.sessions.signOut")}
      </Button>
    </li>
  );
}

/**
 * Settings › Browser access (desktop only, spec `settings` "Browser access section"): the app
 * serves its own library to a phone, a TV or another computer on the home network. Off by
 * default; it needs a password first. While it runs, the section shows the addresses with Copy
 * and a QR code, the port, who is signed in, and the tray and sign-in options.
 */
export function BrowserAccessSection() {
  const [access, setAccess] = useState<BrowserAccess | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const switchId = useId();
  const lanId = useId();
  const trayId = useId();
  const loginId = useId();

  const reload = useCallback(async () => {
    try {
      setAccess(await currentBrowserAccessBackend().get());
    } catch (cause) {
      setError(messageOf(cause));
    }
  }, []);

  useEffect(() => {
    let live = true;
    const read = () => {
      currentBrowserAccessBackend()
        .get()
        .then((value) => {
          if (live) setAccess(value);
        })
        .catch((cause: unknown) => {
          if (live) setError(messageOf(cause));
        });
    };
    read();
    const timer = window.setInterval(read, REFRESH_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, []);

  /** Runs one change; the answer is the new state. Returns whether it worked. */
  const run = useCallback(
    async (work: () => Promise<BrowserAccess>, done?: string): Promise<boolean> => {
      setBusy(true);
      setError(null);
      try {
        setAccess(await work());
        if (done !== undefined) setStatus(done);
        return true;
      } catch (cause) {
        setError(messageOf(cause));
        void reload();
        return false;
      } finally {
        setBusy(false);
      }
    },
    [reload],
  );

  const backend = currentBrowserAccessBackend();
  const running = access?.state === "running";
  const first = access?.addresses[0];

  return (
    <SettingsSection
      id="browser-access"
      title={t("settingsPage.browserAccess.title")}
      hint={t("settingsPage.browserAccess.hint")}
    >
      {access === null ? (
        <FormRow label={t("settingsPage.browserAccess.title")}>
          <span className="text-(length:--fs-13) text-text-muted">…</span>
        </FormRow>
      ) : (
        <>
          <FormRow
            label={<span id={switchId}>{t("settingsPage.browserAccess.switch")}</span>}
            hint={t("settingsPage.browserAccess.switch.hint")}
          >
            <div className="flex items-center gap-3">
              <Switch
                checked={access.enabled}
                aria-labelledby={switchId}
                disabled={busy || !access.available || (!access.enabled && !access.hasPassword)}
                onCheckedChange={(on) => {
                  void run(
                    () => backend.setEnabled(on),
                    on
                      ? t("settingsPage.browserAccess.started")
                      : t("settingsPage.browserAccess.stopped"),
                  );
                }}
              />
              <span
                data-state={access.state}
                className={
                  access.state === "running"
                    ? "text-(length:--fs-13) text-ok"
                    : access.state === "portInUse" || access.state === "error"
                      ? "text-(length:--fs-13) text-danger"
                      : "text-(length:--fs-13) text-text-muted"
                }
              >
                {stateText(access.state)}
              </span>
            </div>
            {access.message !== null ? (
              <p role="alert" className="m-0 mt-1.5 text-(length:--fs-12) text-danger">
                {access.message}
              </p>
            ) : null}
          </FormRow>
          {access.windows && !running ? (
            <div className={ROW_BLOCK}>
              <p className="m-0 text-(length:--fs-12) text-text-muted">
                {t("settingsPage.browserAccess.firewall")}
              </p>
            </div>
          ) : null}
          <PasswordRow access={access} run={run} />
          {running ? (
            <FormRow
              label={t("settingsPage.browserAccess.addresses")}
              hint={t("settingsPage.browserAccess.addresses.hint")}
              align="start"
            >
              <div className="flex flex-wrap items-start gap-5">
                <ul className="m-0 flex list-none flex-col gap-2 p-0">
                  {access.addresses.map((address) => (
                    <li key={address} className="flex items-center gap-2">
                      <code className="font-mono text-(length:--fs-13) select-text">{address}</code>
                      <CopyButton text={address} />
                    </li>
                  ))}
                </ul>
                {first !== undefined && !first.includes("localhost") ? (
                  <figure className="m-0 flex flex-col items-center gap-1">
                    <QrCode
                      value={first}
                      label={t("settingsPage.browserAccess.qr", { address: first })}
                    />
                    <figcaption className="text-(length:--fs-12) text-text-muted">
                      {t("settingsPage.browserAccess.qr.caption")}
                    </figcaption>
                  </figure>
                ) : null}
              </div>
              {access.windows ? (
                <p className="m-0 mt-2 text-(length:--fs-12) text-text-muted">
                  {t("settingsPage.browserAccess.firewall")}
                </p>
              ) : null}
            </FormRow>
          ) : null}
          {/* Keyed by the port, so the field starts from the port the server answered with. */}
          <PortRow key={access.port} access={access} run={run} />
          <FormRow
            label={<span id={lanId}>{t("settingsPage.browserAccess.lanOnly")}</span>}
            hint={
              access.lanOnly
                ? t("settingsPage.browserAccess.lanOnly.hint")
                : t("settingsPage.browserAccess.lanOnly.off")
            }
          >
            <Switch
              checked={access.lanOnly}
              aria-labelledby={lanId}
              disabled={busy}
              onCheckedChange={(on) => void run(() => backend.setLanOnly(on))}
            />
          </FormRow>
          {running ? (
            <FormRow
              label={t("settingsPage.browserAccess.sessions")}
              hint={t("settingsPage.browserAccess.sessions.hint")}
              align="start"
            >
              {access.sessions.length === 0 ? (
                <span className="text-(length:--fs-13) text-text-muted">
                  {t("settingsPage.browserAccess.sessions.none")}
                </span>
              ) : (
                <div className="flex flex-col gap-2">
                  <ul className="m-0 flex list-none flex-col p-0">
                    {access.sessions.map((session) => (
                      <SessionLine
                        key={session.handle}
                        session={session}
                        onSignOut={() =>
                          void run(
                            () => backend.signOut(session.handle),
                            t("settingsPage.browserAccess.sessions.signedOutOne"),
                          )
                        }
                      />
                    ))}
                  </ul>
                  <div>
                    <Button
                      variant="danger"
                      onClick={() =>
                        void run(
                          () => backend.signOut(null),
                          t("settingsPage.browserAccess.sessions.signedOutAll"),
                        )
                      }
                    >
                      {t("settingsPage.browserAccess.sessions.signOutAll")}
                    </Button>
                  </div>
                </div>
              )}
            </FormRow>
          ) : null}
          <FormRow
            label={<span id={trayId}>{t("settingsPage.browserAccess.tray")}</span>}
            hint={t("settingsPage.browserAccess.tray.hint")}
          >
            <Switch
              checked={access.keepInTray}
              aria-labelledby={trayId}
              disabled={busy}
              onCheckedChange={(on) => void run(() => backend.setTray(on))}
            />
          </FormRow>
          <FormRow
            label={<span id={loginId}>{t("settingsPage.browserAccess.login")}</span>}
            hint={t("settingsPage.browserAccess.login.hint")}
          >
            <Switch
              checked={access.startAtLogin}
              aria-labelledby={loginId}
              disabled={busy}
              onCheckedChange={(on) => void run(() => backend.setStartAtLogin(on))}
            />
          </FormRow>
          <div className={ROW_BLOCK}>
            <p className="m-0 text-(length:--fs-12) text-text-muted">
              {t("settingsPage.browserAccess.help", { computer: access.computer })}
            </p>
          </div>
        </>
      )}
      {error !== null ? (
        <p role="alert" className="m-0 px-4 py-2 text-(length:--fs-12) text-danger">
          {error}
        </p>
      ) : null}
      <p role="status" className="sr-only">
        {status}
      </p>
    </SettingsSection>
  );
}
