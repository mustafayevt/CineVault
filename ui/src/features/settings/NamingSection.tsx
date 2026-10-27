import { useEffect, useRef, useState } from "react";

import { Button } from "../../components/Button";
import { Field } from "../../components/Field";
import { Popover } from "../../components/Popover";
import { SegmentedControl } from "../../components/SegmentedControl";
import { t } from "../../i18n/catalog";
import { FormRow, SettingsSection } from "./FormParts";
import {
  COLON_SETTING_KEY,
  colonStyleOf,
  followColonStyle,
  insertToken,
  tokensOf,
} from "./namingModel";
import type { ColonStyle, NamingField } from "./namingModel";
import { useNamingTemplates } from "./useNamingTemplates";
import type { NamingTemplates } from "./useNamingTemplates";
import { useSettingsStore } from "./settingsStore";

const EXTENSION = /\.[A-Za-z0-9]{2,4}$/;

/** The live examples under the fields, from the plan for the templates being edited. */
function examplesOf(naming: NamingTemplates, colon: ColonStyle) {
  const preview = naming.preview;
  if (preview === undefined) return { folder: null, file: null, episode: null };
  const movies = preview.examples.filter((example) => example.kind !== "episode");
  const pick = movies.find((example) => example.kind === "colon") ?? movies[0];
  // The backend renders the episode template on a built-in sample show, so there is an example
  // whether or not the library holds a show.
  const episode = preview.examples.find((example) => example.kind === "episode");
  return {
    folder: pick === undefined ? null : followColonStyle(pick.folder, pick.title, colon),
    file:
      pick === undefined
        ? null
        : followColonStyle(pick.file, pick.title, colon) +
          (EXTENSION.test(pick.file) ? "" : ".mkv"),
    episode:
      episode === undefined ? null : episode.file + (EXTENSION.test(episode.file) ? "" : ".mkv"),
  };
}

function Example({ text }: { text: string | null }) {
  return (
    <div className="text-(length:--fs-12) text-text-2">
      {t("settingsPage.naming.example")}{" "}
      {/* The sample line keeps its height while the plan is read, so nothing shifts. */}
      <span className="cv-selectable font-mono text-text">{text ?? " "}</span>
    </div>
  );
}

/**
 * Settings, Naming (spec `renamer` "Naming templates in Settings" and "Colon in titles option"):
 * the Movie folder, Movie file and TV episode templates as mono text fields with a live example
 * under each, the token buttons under each field that insert at its cursor, the Style control that fills all three at once, and how a colon
 * in a title is written.
 */
export function NamingSection() {
  const naming = useNamingTemplates();
  const colon = colonStyleOf(useSettingsStore((state) => state.values[COLON_SETTING_KEY]));
  // The preset waiting for the answer to "Replace your custom templates?".
  const [asking, setAsking] = useState<string | null>(null);
  const [styleProblem, setStyleProblem] = useState<string | null>(null);
  const inputs = useRef<Partial<Record<NamingField, HTMLInputElement | null>>>({});
  const caret = useRef<{ field: NamingField; at: number } | null>(null);

  // After an insertion the controlled field is drawn again: put the caret behind the token.
  useEffect(() => {
    const pending = caret.current;
    if (pending === null) return;
    caret.current = null;
    const input = inputs.current[pending.field];
    input?.focus();
    input?.setSelectionRange(pending.at, pending.at);
  });

  const insert = (field: NamingField, label: string) => {
    const input = inputs.current[field];
    const text = naming.valueOf(field);
    const start = input?.selectionStart ?? text.length;
    const end = input?.selectionEnd ?? text.length;
    const next = insertToken(text, start, end, label);
    caret.current = { field, at: next.caret };
    naming.change(field, next.text, true);
  };

  const presets = naming.styles.filter((style) => style.builtin);
  const hasCustom = naming.styles.some((style) => !style.builtin);
  const active = naming.activeStyle;
  const customActive = active !== undefined && !active.builtin;
  const customStyle = naming.styles.find((style) => !style.builtin);
  const asked = presets.find((preset) => preset.id === asking);

  const choose = async (id: string, discard?: string) => {
    setAsking(null);
    setStyleProblem(await naming.choose(id, discard));
  };

  const examples = examplesOf(naming, colon);
  const row = (
    field: NamingField,
    label: string,
    hint: string | undefined,
    example: string | null,
  ) => (
    <FormRow label={label} labelId={`naming-${field}`} hint={hint} align="start">
      <div className="flex flex-col gap-1.5">
        <Field
          mono
          ref={(node) => {
            inputs.current[field] = node;
          }}
          aria-label={t("settingsPage.naming.template", { name: label })}
          spellCheck={false}
          autoComplete="off"
          disabled={!naming.ready}
          value={naming.valueOf(field)}
          error={naming.problemOf(field) ?? undefined}
          onChange={(event) => {
            naming.change(field, event.target.value);
          }}
        />
        <Example text={example} />
        <div
          role="group"
          aria-label={t("settingsPage.naming.tokens", { name: label })}
          className="flex flex-wrap gap-[5px]"
        >
          {tokensOf(field).map((token) => (
            <button
              key={token.label}
              type="button"
              // Keeps the caret in the field while the button is pressed.
              onMouseDown={(event) => {
                event.preventDefault();
              }}
              onClick={() => {
                insert(field, token.label);
              }}
              className="cv-hoverable rounded-(--r-4) border border-solid border-line-edge bg-transparent px-1.5 py-px font-mono text-(length:--fs-11) text-text-2 hover:bg-surface-raised hover:text-text active:bg-surface-pressed"
            >
              {`{${token.label}}`}
            </button>
          ))}
        </div>
      </div>
    </FormRow>
  );

  return (
    <SettingsSection id="naming" title={t("settingsPage.naming.title")}>
      <FormRow
        label={t("settingsPage.naming.style")}
        hint={t("settingsPage.naming.style.hint")}
        labelId="naming-style"
        align="start"
      >
        <div className="flex flex-col gap-1.5">
          <Popover
            open={asked !== undefined}
            onOpenChange={(open) => {
              // Only the answer closes it; the wrapper is no button.
              if (!open) setAsking(null);
            }}
            align="start"
            aria-label={t("settingsPage.naming.replace.title")}
            className="w-[340px]"
            trigger={
              <div className="inline-flex self-start">
                <SegmentedControl<string>
                  aria-label={t("settingsPage.naming.style")}
                  value={active === undefined ? "" : active.builtin ? active.id : "custom"}
                  options={[
                    ...presets.map((preset) => ({ value: preset.id, label: preset.name })),
                    ...(hasCustom
                      ? [{ value: "custom", label: t("settingsPage.naming.style.custom") }]
                      : []),
                  ]}
                  onChange={(value) => {
                    if (!naming.ready) return;
                    if (value === "custom") {
                      if (!customActive && customStyle !== undefined) void choose(customStyle.id);
                    } else if (value !== active?.id) {
                      if (customActive) setAsking(value);
                      else void choose(value);
                    }
                  }}
                />
              </div>
            }
          >
            <p className="m-0 text-(length:--fs-13) text-text">
              {t("settingsPage.naming.replace.body", { name: asked?.name ?? "" })}
            </p>
            <div className="mt-3 flex justify-end gap-2">
              <Button
                size="sm"
                onClick={() => {
                  setAsking(null);
                }}
              >
                {t("settingsPage.naming.replace.cancel")}
              </Button>
              <Button
                size="sm"
                variant="primary"
                onClick={() => {
                  if (asked !== undefined) void choose(asked.id, active?.id);
                }}
              >
                {t("settingsPage.naming.replace.go")}
              </Button>
            </div>
          </Popover>
          {styleProblem === null ? null : (
            <div role="alert" className="text-(length:--fs-12) text-danger">
              {styleProblem}
            </div>
          )}
        </div>
      </FormRow>
      {row(
        "movie_folder",
        t("settingsPage.naming.movieFolder"),
        t("settingsPage.naming.movieFolder.hint"),
        examples.folder,
      )}
      {row(
        "movie_file",
        t("settingsPage.naming.movieFile"),
        t("settingsPage.naming.movieFile.hint"),
        examples.file,
      )}
      {row("episode_file", t("settingsPage.naming.episode"), undefined, examples.episode)}
      <FormRow label={t("settingsPage.naming.colon")}>
        <SegmentedControl<ColonStyle>
          aria-label={t("settingsPage.naming.colon")}
          value={colon}
          options={[
            { value: "remove", label: t("settingsPage.naming.colon.remove") },
            { value: "dash", label: t("settingsPage.naming.colon.dash") },
            { value: "lookalike", label: t("settingsPage.naming.colon.lookalike") },
          ]}
          onChange={(value) => {
            useSettingsStore.getState().set(COLON_SETTING_KEY, value);
          }}
        />
      </FormRow>
    </SettingsSection>
  );
}
