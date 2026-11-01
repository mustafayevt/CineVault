import {
  Eye,
  EyeOff,
  Folder,
  Info,
  KeyRound,
  Lock,
  LoaderCircle,
  Plus,
  TriangleAlert,
} from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import type { FormEvent, ReactNode } from "react";

import { Button } from "../../components/Button";
import { Card } from "../../components/Card";
import { Checkbox } from "../../components/Checkbox";
import { Field } from "../../components/Field";
import { Icon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { t } from "../../i18n/catalog";
import type { SourceKind } from "../library-sources/sourcesBackend";
import { useEscapeLayer } from "../shell/escStack";
import { CopyCommand } from "./CopyCommand";
import { libraryFolderPath, parseShareAddress, shareName } from "./shareAddress";
import type { ShareAddress } from "./shareAddress";
import { ShareRequestError, currentSharesBackend } from "./sharesBackend";
import type { NetworkShare, ShareAdded, ShareSupport } from "./sharesBackend";

/** What the page is for: a new share (and the library its folder goes to) or an offline one. */
export type ShareMode =
  { kind: "connect"; library: SourceKind | null } | { kind: "again"; share: NetworkShare };

/** What was connected. */
export type ShareDone =
  { kind: "added"; added: ShareAdded } | { kind: "again"; share: NetworkShare };

/** What went wrong, told next to the field concerned or in a banner above the buttons. */
export interface ShareProblem {
  field: "address" | "user" | "password" | "banner";
  title: string;
  body?: string;
  /** Offer "Connect without remembering". */
  withoutRemembering?: boolean;
}

function problemOf(error: unknown, address: ShareAddress | null): ShareProblem | null {
  if (!(error instanceof ShareRequestError)) {
    return { field: "banner", title: error instanceof Error ? error.message : String(error) };
  }
  const server = address?.server ?? "";
  const share = address === null ? "" : address.share;
  switch (error.code) {
    case "cancelled":
      return null;
    case "wrong_login":
      return { field: "password", title: t("networkShare.error.wrongLogin") };
    case "unreachable":
      return { field: "address", title: t("networkShare.error.unreachable", { server }) };
    case "share_not_found":
      return { field: "address", title: t("networkShare.error.shareNotFound", { share, server }) };
    case "folder_not_found":
      return {
        field: "address",
        title: t("networkShare.error.folderNotFound", {
          folder: address?.folder.join("\\") ?? "",
          share: address === null ? "" : shareName(address),
        }),
      };
    case "invalid_address":
      return { field: "address", title: error.message };
    case "invalid_user":
      return { field: "user", title: error.message };
    case "invalid_password":
      return { field: "password", title: error.message };
    case "other_user_connected":
      return {
        field: "banner",
        title: t("networkShare.error.otherUser", { server }),
        body: t("networkShare.error.otherUserBody"),
      };
    case "no_keyring":
      return {
        field: "banner",
        title: t("networkShare.error.noKeyring"),
        body: t("networkShare.error.noKeyringBody"),
        withoutRemembering: true,
      };
    default:
      return { field: "banner", title: error.message };
  }
}

interface RowProps {
  label: string;
  /** The id of the control the label names. */
  htmlFor?: string;
  children: ReactNode;
}

/** A row of the form: the label in a 150px column, the control beside it. */
function Row({ label, htmlFor, children }: RowProps) {
  return (
    <div className="grid grid-cols-[150px_minmax(0,1fr)] items-start gap-5 border-t border-solid border-line-row px-4 py-3 first:border-t-0">
      <label htmlFor={htmlFor} className="pt-1.5 font-(weight:--fw-500)">
        {label}
      </label>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

/** A warning above the buttons: what is wrong, what to do, and optionally a way on. */
function Banner({
  icon,
  problem,
  action,
}: {
  icon: typeof TriangleAlert;
  problem: ShareProblem;
  action?: ReactNode;
}) {
  return (
    <div
      role="alert"
      className="flex items-center gap-2.5 rounded-(--r-10) bg-warn-tint px-3 py-2.5"
    >
      <Icon icon={icon} className="shrink-0 self-start text-warn" />
      <div className="min-w-0 flex-1">
        <div className="font-(weight:--fw-600)">{problem.title}</div>
        {problem.body === undefined ? null : <div className="text-text-2">{problem.body}</div>}
      </div>
      {action}
    </div>
  );
}

/** The sentence about an address that is typed or pasted: the two forms, in mono. */
function AddressHint({ id }: { id: string }) {
  const unc = "\\\\nas\\movies";
  const smb = "smb://nas/movies";
  const parts = t("networkShare.address.hint", { unc, smb }).split(
    /(\\\\nas\\movies|smb:\/\/nas\/movies)/,
  );
  return (
    <p id={id} className="type-small m-0 mt-1 text-text-muted">
      {parts.map((part, index) =>
        part === unc || part === smb ? (
          <span key={index} className="font-mono">
            {part}
          </span>
        ) : (
          part
        ),
      )}
    </p>
  );
}

interface NetworkShareFormProps {
  mode: ShareMode;
  support: ShareSupport;
  /** The share is connected (and its folder added). */
  onDone: (done: ShareDone) => void;
  /** Leave without connecting: Cancel and Esc. */
  onCancel: () => void;
  /** Linux without share mounting: the way on is "Add folder…". */
  onAddFolder: () => void;
  /** What the form holds when it opens; a specimen of the gallery shows each state with it. */
  start?: ShareFormStart;
  /** The page itself: it takes the focus and Esc leaves it. Off for a specimen of the gallery. */
  standalone?: boolean;
}

/** What a form holds when it opens, besides what its mode fills in. */
export interface ShareFormStart {
  address?: string;
  user?: string;
  password?: string;
  busy?: boolean;
  problem?: ShareProblem;
}

const MOUNT_COMMAND = [
  "sudo mkdir -p /mnt/SHARE",
  "sudo mount -t cifs //SERVER/SHARE /mnt/SHARE -o username=USER,uid=$(id -u),gid=$(id -g)",
] as const;

/**
 * The body of the Network share page (spec `network-shares`): Address, the library folder it
 * gives, User, Password with Show and "Remember the password", then Cancel and "Connect and add".
 * While it connects the fields are read-only and Esc stops the tool; a refusal is told inline,
 * next to the field concerned, and keeps what was typed. For a share that is offline the Address
 * is fixed and the password has the focus. A Linux desktop without share mounting gets the
 * system's own way instead of the fields.
 */
export function NetworkShareForm({
  mode,
  support,
  onDone,
  onCancel,
  onAddFolder,
  start,
  standalone = true,
}: NetworkShareFormProps) {
  const again = mode.kind === "again";
  const ids = useId();
  const [address, setAddress] = useState(start?.address ?? (again ? mode.share.address : ""));
  const [user, setUser] = useState(start?.user ?? (again ? mode.share.user : ""));
  const [password, setPassword] = useState(start?.password ?? "");
  const [remember, setRemember] = useState(again ? mode.share.remember : true);
  const [show, setShow] = useState(false);
  const [busy, setBusy] = useState(start?.busy ?? false);
  const [problem, setProblem] = useState<ShareProblem | null>(start?.problem ?? null);
  const addressField = useRef<HTMLInputElement>(null);
  const passwordField = useRef<HTMLInputElement>(null);
  const parsed = parseShareAddress(address);

  useEffect(() => {
    if (standalone) (again ? passwordField : addressField).current?.focus();
  }, [again, standalone, support.supported]);

  const connect = async (keep: boolean) => {
    setBusy(true);
    setProblem(null);
    const backend = currentSharesBackend();
    try {
      if (mode.kind === "again") {
        const share = await backend.connectAgain({
          id: mode.share.id,
          user,
          password,
          remember: keep,
        });
        onDone({ kind: "again", share });
      } else {
        const added = await backend.connect({
          address,
          kind: mode.library,
          user,
          password,
          remember: keep,
        });
        onDone({ kind: "added", added });
      }
    } catch (error: unknown) {
      setProblem(problemOf(error, parsed));
      setBusy(false);
    }
  };

  // Esc and Cancel stop a connect that is running; otherwise they leave the page.
  const escape = () => {
    if (busy) void currentSharesBackend().cancel();
    else onCancel();
    return true;
  };
  useEscapeLayer("picker", standalone, escape);
  // A field holding text keeps Esc for itself, but stopping a connect discards nothing.
  useEffect(() => {
    if (!busy) return undefined;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      void currentSharesBackend().cancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [busy]);

  if (!support.supported) {
    return (
      <div className="flex flex-col gap-4">
        <Card className="flex flex-col gap-4 p-5">
          <div className="flex items-center gap-2.5">
            <span className="flex size-7 shrink-0 items-center justify-center rounded-(--r-6) bg-surface-active text-text-2">
              <Icon icon={Info} />
            </span>
            <h2 className="m-0 text-(length:--fs-14) font-(weight:--fw-600)">
              {t("networkShare.unsupported.title")}
            </h2>
          </div>
          <p className="m-0 text-text-2">{t("networkShare.unsupported.body")}</p>
          <div>
            <div className="mb-1.5 font-(weight:--fw-500) text-text-2">
              {t("networkShare.unsupported.terminal")}
            </div>
            <CopyCommand lines={MOUNT_COMMAND} />
            <p className="type-small m-0 mt-1 text-text-muted">
              {t("networkShare.unsupported.hint")}
            </p>
          </div>
        </Card>
        <div className="flex justify-end gap-2">
          <Button size="lg" shortcut="Esc" onClick={onCancel}>
            {t("networkShare.cancel")}
          </Button>
          <Button variant="primary" size="lg" icon={Plus} onClick={onAddFolder}>
            {t("networkShare.unsupported.add")}
          </Button>
        </div>
      </div>
    );
  }

  const system = support.system;
  const ready = again
    ? user.trim() !== ""
    : parsed !== null && (user.trim() !== "" || parsed.user !== null);
  const introText =
    mode.kind === "connect"
      ? t(`networkShare.intro.${mode.library ?? "any"}`, {
          app: t(`networkShare.app.${system}`),
        })
      : t(
          mode.share.reason === "wrong_password" || mode.share.reason === "password_needed"
            ? `networkShare.introAgain.${mode.share.reason}`
            : "networkShare.introAgain.other",
          { user: mode.share.user, address: mode.share.address },
        );
  const preview = parsed === null ? null : libraryFolderPath(parsed, support);
  const readOnly = busy;
  const fixed = "bg-surface-well! text-text-2";
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (ready && !busy) void connect(remember);
  };

  return (
    <form onSubmit={submit} noValidate className="flex flex-col gap-4">
      <p className="m-0 max-w-[640px] text-text-2">{introText}</p>
      <Card className="overflow-hidden p-0">
        <Row label={t("networkShare.address")} htmlFor={`${ids}-address`}>
          <Field
            ref={addressField}
            id={`${ids}-address`}
            mono
            value={address}
            placeholder={t("networkShare.address.placeholder")}
            readOnly={readOnly || again}
            autoComplete="off"
            spellCheck={false}
            aria-describedby={
              problem?.field === "address" || again ? undefined : `${ids}-address-hint`
            }
            error={problem?.field === "address" ? problem.title : undefined}
            boxClassName={readOnly || again ? fixed : undefined}
            trailing={
              again ? (
                <span
                  role="img"
                  aria-label={t("networkShare.address.fixed")}
                  className="shrink-0 text-text-muted"
                >
                  <Icon icon={Lock} size={14} />
                </span>
              ) : undefined
            }
            onChange={(event) => {
              setAddress(event.target.value);
              const named = parseShareAddress(event.target.value)?.user;
              if (named !== null && named !== undefined && user === "") setUser(named);
            }}
          />
          {again || problem?.field === "address" ? null : (
            <AddressHint id={`${ids}-address-hint`} />
          )}
        </Row>
        <Row label={t("networkShare.folder")}>
          <div
            className={`flex h-[30px] items-center gap-2 ${preview === null ? "text-text-disabled" : "text-text-2"}`}
          >
            <Icon icon={Folder} />
            {preview === null ? (
              <span>{t("networkShare.folder.empty")}</span>
            ) : (
              <span className="cv-selectable truncate font-mono text-(length:--fs-12)">
                {preview}
              </span>
            )}
          </div>
        </Row>
        <Row label={t("networkShare.user")} htmlFor={`${ids}-user`}>
          <Field
            id={`${ids}-user`}
            value={user}
            placeholder={t("networkShare.user.placeholder")}
            readOnly={readOnly}
            autoComplete="off"
            spellCheck={false}
            description={t("networkShare.user.hint")}
            error={problem?.field === "user" ? problem.title : undefined}
            boxClassName={readOnly ? fixed : undefined}
            onChange={(event) => setUser(event.target.value)}
          />
        </Row>
        <Row label={t("networkShare.password")} htmlFor={`${ids}-password`}>
          <Field
            ref={passwordField}
            id={`${ids}-password`}
            type={show ? "text" : "password"}
            value={password}
            readOnly={readOnly}
            autoComplete="off"
            error={problem?.field === "password" ? problem.title : undefined}
            boxClassName={readOnly ? fixed : undefined}
            trailing={
              <IconButton
                size="sm"
                icon={show ? EyeOff : Eye}
                aria-label={
                  show ? t("networkShare.password.hide") : t("networkShare.password.show")
                }
                aria-pressed={show}
                onClick={() => setShow(!show)}
              />
            }
            onChange={(event) => setPassword(event.target.value)}
          />
        </Row>
        <div className="grid grid-cols-[150px_minmax(0,1fr)] items-start gap-5 border-t border-solid border-line-row px-4 py-3">
          <span aria-hidden="true" />
          <div>
            <Checkbox checked={remember} disabled={busy} onCheckedChange={setRemember}>
              <span className="font-(weight:--fw-500)">{t("networkShare.remember")}</span>
            </Checkbox>
            <p className="type-small m-0 mt-0.5 ml-[22px] text-text-muted">
              {t(`networkShare.store.${system}`)}
            </p>
          </div>
        </div>
      </Card>
      {problem?.field === "banner" ? (
        <Banner
          icon={problem.withoutRemembering === true ? KeyRound : TriangleAlert}
          problem={problem}
          action={
            problem.withoutRemembering === true ? (
              <Button
                onClick={() => {
                  setRemember(false);
                  void connect(false);
                }}
              >
                {t("networkShare.error.withoutRemembering")}
              </Button>
            ) : undefined
          }
        />
      ) : null}
      <div className="flex justify-end gap-2">
        <Button size="lg" shortcut="Esc" onClick={escape}>
          {t("networkShare.cancel")}
        </Button>
        {busy ? (
          <Button
            variant="primary"
            size="lg"
            type="submit"
            icon={LoaderCircle}
            aria-busy="true"
            className="[&_svg]:animate-spin"
            onClick={(event) => event.preventDefault()}
          >
            {t("networkShare.connecting")}
          </Button>
        ) : (
          <Button
            variant="primary"
            size="lg"
            type="submit"
            {...(ready
              ? {}
              : {
                  disabledReason: t("networkShare.needs"),
                  enabledBy: t("networkShare.needsFix"),
                })}
          >
            {again ? t("networkShare.connectAgain") : t("networkShare.connect")}
          </Button>
        )}
      </div>
    </form>
  );
}
