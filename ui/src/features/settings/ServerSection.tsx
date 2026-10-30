import { useCallback, useEffect, useState } from "react";

import { Button } from "../../components/Button";
import { FormRow, SettingsSection } from "./FormParts";

interface ServerInfo {
  version: string;
  bind: string;
  port: number;
  sessions: number;
  keys: string;
}

function csrf(): string {
  const value = (window as unknown as Record<string, unknown>).__CINEVAULT_CSRF__;
  return typeof value === "string" ? value : "";
}

async function readInfo(): Promise<ServerInfo> {
  const response = await fetch("/api/server/info", { credentials: "same-origin" });
  if (!response.ok) throw new Error("The server did not answer.");
  return (await response.json()) as ServerInfo;
}

/**
 * The Server settings, shown only when the interface is served by the browser server. The address
 * is read-only (it comes from the environment), the password is changed inline, and signing out
 * every session keeps this one.
 */
export function ServerSection() {
  const [info, setInfo] = useState<ServerInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [changing, setChanging] = useState(false);
  const [current, setCurrent] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [message, setMessage] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setInfo(await readInfo());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, []);

  useEffect(() => {
    let active = true;
    readInfo()
      .then((value) => {
        if (active) setInfo(value);
      })
      .catch((cause: unknown) => {
        if (active) setError(cause instanceof Error ? cause.message : String(cause));
      });
    return () => {
      active = false;
    };
  }, []);

  const change = async () => {
    setMessage(null);
    setError(null);
    const response = await fetch("/api/server/password", {
      method: "POST",
      credentials: "same-origin",
      headers: { "content-type": "application/json", "x-cinevault-csrf": csrf() },
      body: JSON.stringify({ current, password, confirm }),
    });
    const body = (await response.json()) as { message?: string };
    if (!response.ok) {
      setError(body.message ?? "The password could not be changed.");
      return;
    }
    setChanging(false);
    setCurrent("");
    setPassword("");
    setConfirm("");
    setMessage("The password was changed. Every other session was signed out.");
    void reload();
  };

  const signOutAll = async () => {
    await fetch("/api/server/sign-out-all", {
      method: "POST",
      credentials: "same-origin",
      headers: { "x-cinevault-csrf": csrf() },
    });
    setMessage("Every other session was signed out.");
    void reload();
  };

  return (
    <SettingsSection id="server" title="Server">
      <FormRow label="Address" hint="From CINEVAULT_WEB_BIND and CINEVAULT_WEB_PORT.">
        <code className="text-(length:--fs-13)">
          {info === null ? "…" : `http://${info.bind}:${info.port}`}
        </code>
      </FormRow>
      <FormRow label="Version">
        <span className="text-(length:--fs-13)">{info?.version ?? "…"}</span>
      </FormRow>
      <FormRow label="Signed-in sessions">
        <span className="text-(length:--fs-13)">{info?.sessions ?? "…"}</span>
      </FormRow>
      <FormRow
        label="Provider keys"
        hint={
          info?.keys === "locked"
            ? "The master key is missing. Set CINEVAULT_SECRET_KEY_FILE and restart."
            : "Kept in the server's encrypted secrets file."
        }
      >
        <span className="text-(length:--fs-13)">
          {info?.keys === "locked" ? "Locked" : "Configured"}
        </span>
      </FormRow>
      <FormRow label="Password" align="start">
        {!changing ? (
          <Button onClick={() => setChanging(true)}>Change password</Button>
        ) : (
          <div className="flex max-w-[360px] flex-col gap-2">
            <input
              type="password"
              autoComplete="current-password"
              placeholder="Current password"
              value={current}
              onChange={(event) => setCurrent(event.target.value)}
              className="w-full rounded-(--r-8) border border-solid border-line-section bg-surface-well px-2.5 py-1.5 text-(length:--fs-13)"
            />
            <input
              type="password"
              autoComplete="new-password"
              placeholder="New password (at least 12 characters)"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              className="w-full rounded-(--r-8) border border-solid border-line-section bg-surface-well px-2.5 py-1.5 text-(length:--fs-13)"
            />
            <input
              type="password"
              autoComplete="new-password"
              placeholder="Confirm the new password"
              value={confirm}
              onChange={(event) => setConfirm(event.target.value)}
              className="w-full rounded-(--r-8) border border-solid border-line-section bg-surface-well px-2.5 py-1.5 text-(length:--fs-13)"
            />
            <div className="flex gap-2">
              <Button variant="primary" onClick={() => void change()}>
                Change password
              </Button>
              <Button onClick={() => setChanging(false)}>Cancel</Button>
            </div>
          </div>
        )}
      </FormRow>
      <FormRow
        label="Sessions"
        hint="Signing out every session keeps this browser signed in."
      >
        <Button onClick={() => void signOutAll()}>Sign out every other session</Button>
      </FormRow>
      {error !== null ? (
        <p role="alert" className="px-4 py-2 text-(length:--fs-12) text-status-problem">
          {error}
        </p>
      ) : null}
      {message !== null ? (
        <p role="status" className="px-4 py-2 text-(length:--fs-12) text-text-muted">
          {message}
        </p>
      ) : null}
    </SettingsSection>
  );
}
