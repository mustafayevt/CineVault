import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";

import { Button } from "../../components/Button";
import { Field } from "../../components/Field";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import type { ProviderCredentials } from "./providersBackend";
import type { ProviderKeys } from "./useProviderKeys";

interface KeyFieldProps {
  /** `undefined` while the list is read, or where there is no credential store. */
  provider: ProviderCredentials | undefined;
  keys: ProviderKeys;
  /** The field's accessible name: "TMDB key". */
  label: string;
  /** Called after a typed key passed Test and was saved. */
  onSaved?: () => void;
  /** Puts the cursor in the field once the provider is known (a link that asks for this key). */
  focusOnShow?: boolean;
}

/** The field a key is typed into: the first one the provider requires. */
function keyFieldOf(provider: ProviderCredentials): string {
  return (provider.fields.find((field) => field.required) ?? provider.fields[0])?.key ?? "api_key";
}

/**
 * A provider key as one line: the masked field, what the saved
 * key does ("Works", "Rejected"), Show and Test. Test asks the provider about the typed key, or
 * the saved one when nothing is typed, and saves a typed key that passes. A saved key is never
 * read back, so the field then shows dots and the line offers Remove (with Undo).
 */
export function KeyField({ provider, keys, label, onSaved, focusOnShow = false }: KeyFieldProps) {
  const [draft, setDraft] = useState("");
  const [reveal, setReveal] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const known = provider !== undefined;
  // The field is disabled until the provider list has been read, so focus waits for it.
  useEffect(() => {
    if (focusOnShow && known) input.current?.focus();
  }, [focusOnShow, known]);

  if (provider === undefined) {
    return (
      <Field
        mono
        disabled
        aria-label={label}
        placeholder={keys.loading ? "" : t("settingsPage.keys.unavailable")}
      />
    );
  }

  const typed = draft.trim();
  const saved = provider.configured;
  const field = keyFieldOf(provider);
  const secret = provider.fields.find((entry) => entry.key === field)?.kind !== "text";

  const run = async () => {
    if (busy || (typed === "" && !saved)) return;
    setBusy(true);
    setProblem(null);
    try {
      const drafts = typed === "" ? {} : { [field]: typed };
      if (provider.testable) {
        const outcome = await keys.test(provider, drafts);
        if (typeof outcome === "string") {
          setProblem(outcome);
          return;
        }
        if (!outcome.ok) {
          setProblem(outcome.message ?? t("settingsPage.keys.rejected", { name: provider.name }));
          return;
        }
      }
      if (typed !== "") {
        const failure = await keys.save(provider, drafts);
        if (failure !== null) {
          setProblem(failure);
          return;
        }
        setDraft("");
        setReveal(false);
        onSaved?.();
      }
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    const failure = await keys.remove(provider);
    if (failure !== null) setProblem(failure);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void run();
    }
  };

  const state = typed !== "" ? null : provider.state;
  return (
    <div data-provider={provider.id} data-state={provider.state} className="flex flex-col gap-1.5">
      <div className="flex items-center gap-2">
        <Field
          mono
          className="min-w-0 flex-1"
          ref={input}
          type={secret && !reveal ? "password" : "text"}
          autoComplete="off"
          spellCheck={false}
          aria-label={label}
          placeholder={saved && typed === "" ? "••••••••••••••••" : t("settingsPage.keys.paste")}
          value={draft}
          onChange={(event) => {
            setDraft(event.target.value);
            // What the last test said was about the old text.
            setProblem(null);
          }}
          onKeyDown={onKeyDown}
        />
        {state === "active" ? <Tag variant="ok">{t("settingsPage.keys.works")}</Tag> : null}
        {state === "rejected" ? (
          <Tag variant="warn">{t("settingsPage.keys.rejectedTag")}</Tag>
        ) : null}
        {typed !== "" && secret ? (
          <Button
            variant="ghost"
            aria-pressed={reveal}
            onClick={() => {
              setReveal(!reveal);
            }}
          >
            {reveal ? t("settingsPage.keys.hide") : t("settingsPage.keys.show")}
          </Button>
        ) : null}
        {typed === "" && saved ? (
          <Button variant="ghost" onClick={() => void remove()}>
            {t("settingsPage.keys.remove")}
          </Button>
        ) : null}
        <Button
          disabled={busy || (typed === "" && !saved)}
          aria-label={t("settingsPage.keys.testFor", { name: provider.name })}
          onClick={() => void run()}
        >
          {busy ? t("settingsPage.keys.testing") : t("settingsPage.keys.test")}
        </Button>
      </div>
      {problem !== null ? (
        <p role="alert" className="m-0 text-(length:--fs-12) text-danger">
          {problem}
        </p>
      ) : null}
    </div>
  );
}
