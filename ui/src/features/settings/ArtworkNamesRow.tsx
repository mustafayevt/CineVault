import { Check } from "lucide-react";
import { useState } from "react";

import { Chip } from "../../components/Chip";
import { Select } from "../../components/Select";
import { t } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";
import { showToast } from "../toasts/toastStore";
import {
  ARTWORK_STYLE_KEY,
  CUSTOM_STYLE,
  OFFERED_STYLES,
  artworkType,
  customFrom,
  customKeys,
  examplesOf,
  isCustom,
  listOf,
  settingKeyOf,
  styleChanges,
  styleOf,
  tableTypes,
  toggled,
  visibleChoices,
} from "./artworkNames";
import type { ArtworkNameChoice, ArtworkNameType } from "./artworkNames";
import { FormRow, ROW_BLOCK } from "./FormParts";
import { useSettingsStore } from "./settingsStore";

type Values = Readonly<Record<string, unknown>>;

function styleLabel(style: string): string {
  return style === CUSTOM_STYLE
    ? t("settingsPage.artworkNames.custom")
    : t(`settings.metadata.artwork-names.option.${style}` as MessageKey);
}

function typeLabel(type: ArtworkNameType): string {
  return t(`settingsPage.artworkNames.type.${type.key}` as MessageKey);
}

/** A choice as the row shows it: `<name>-poster`. */
function choiceLabel(choice: ArtworkNameChoice): string {
  return choice.label.replace("{name}", t("settingsPage.artworkNames.name"));
}

/** The values of the style and every per-type key as they are now, for an Undo. */
function snapshotOf(values: Values): Record<string, unknown> {
  return Object.fromEntries(
    [ARTWORK_STYLE_KEY, ...customKeys()].map((key) => [
      key,
      Object.hasOwn(values, key) ? values[key] : undefined,
    ]),
  );
}

const ROW_GRID =
  "grid grid-cols-[124px_minmax(0,1fr)_minmax(0,36%)] items-start gap-x-4 @max-[640px]/pane:grid-cols-1 @max-[640px]/pane:gap-y-1.5";

interface TypeRowProps {
  type: ArtworkNameType;
  list: readonly string[];
  onChange: (type: ArtworkNameType, id: string) => void;
}

/** One artwork type of the custom table: its names to switch on and off, and the files they give. */
function TypeRow({ type, list, onChange }: TypeRowProps) {
  const label = typeLabel(type);
  const choices = visibleChoices(type, list);
  const files = examplesOf(type, list);
  return (
    <div
      role="group"
      aria-label={label}
      data-artwork-type={type.key}
      className={`${ROW_GRID} border-t border-solid border-line-row py-2 first:border-t-0`}
    >
      <div className="pt-1 text-(length:--fs-13) font-(weight:--fw-500)">{label}</div>
      <div className="flex flex-wrap gap-1.5">
        {type.single ? (
          <Select
            className="w-[240px] max-w-full"
            aria-label={t("settingsPage.artworkNames.single", { type: label })}
            options={choices.map((choice) => ({ value: choice.id, label: choiceLabel(choice) }))}
            value={list[0] ?? ""}
            onValueChange={(value) => {
              onChange(type, value);
            }}
          />
        ) : (
          choices.map((choice) => {
            const on = list.includes(choice.id);
            const last = on && list.length === 1;
            return (
              <Chip
                key={choice.id}
                active={on}
                icon={on ? Check : undefined}
                aria-disabled={last || undefined}
                title={last ? t("settingsPage.artworkNames.keepOne") : undefined}
                className="font-mono"
                onClick={() => {
                  if (!last) onChange(type, choice.id);
                }}
              >
                {choiceLabel(choice)}
              </Chip>
            );
          })
        )}
      </div>
      <div
        data-testid={`artwork-files-${type.key}`}
        className="cv-selectable flex min-w-0 flex-col pt-1 font-mono text-(length:--fs-12) text-text-2 [overflow-wrap:anywhere]"
      >
        {files.map((file) => (
          <span key={file}>{file}</span>
        ))}
      </div>
    </div>
  );
}

function TypeTable({
  scope,
  values,
  onChange,
}: {
  scope: "movie" | "tv";
  values: Values;
  onChange: (type: ArtworkNameType, id: string) => void;
}) {
  const title =
    scope === "movie" ? t("settingsPage.artworkNames.movies") : t("settingsPage.artworkNames.tv");
  return (
    <section aria-label={title} className="flex flex-col">
      <div
        aria-hidden="true"
        className={`${ROW_GRID} pb-1 text-(length:--fs-11) font-(weight:--fw-600) tracking-[0.06em] text-text-muted uppercase @max-[640px]/pane:hidden`}
      >
        <span>{title}</span>
        <span>{t("settingsPage.artworkNames.column.names")}</span>
        <span>{t("settingsPage.artworkNames.column.files")}</span>
      </div>
      <div className="hidden pb-1 text-(length:--fs-12) font-(weight:--fw-600) text-text-muted @max-[640px]/pane:block">
        {title}
      </div>
      {tableTypes(scope).map((type) => (
        <TypeRow key={type.key} type={type} list={listOf(type, values)} onChange={onChange} />
      ))}
    </section>
  );
}

/**
 * Settings › Naming › Artwork file names (spec `settings` "Artwork file names in the Naming
 * section"): a style as the quick choice, or Custom with a row per artwork type to choose its
 * names, each with the files it gives for the example movie. Starting Custom writes the names of
 * the style in use, so nothing changes until a row is edited; choosing a style again replaces the
 * custom names, with Undo. After a change a line says that Rename brings the existing artwork to
 * the new names.
 */
export function ArtworkNamesRow() {
  const values = useSettingsStore((state) => state.values);
  const ready = useSettingsStore((state) => state.status === "ready");
  const [changed, setChanged] = useState(false);
  const custom = isCustom(values);
  const style = styleOf(values);
  const current = custom ? CUSTOM_STYLE : style;

  const styles = [
    ...OFFERED_STYLES,
    ...(OFFERED_STYLES.includes(style) ? [] : [style]),
    CUSTOM_STYLE,
  ];

  const choose = (value: string) => {
    const store = useSettingsStore.getState();
    if (value === CUSTOM_STYLE) {
      // The style's own lists, written out: the names stay as they are.
      if (!custom) store.setMany(customFrom(values));
      return;
    }
    if (!custom && value === style) return;
    const previous = snapshotOf(values);
    store.setMany(styleChanges(value));
    setChanged(true);
    if (custom) {
      showToast({
        message: t("settingsPage.artworkNames.replaced", { name: styleLabel(value) }),
        undo: () => {
          useSettingsStore.getState().restore(previous);
        },
      });
    }
  };

  const change = (type: ArtworkNameType, id: string) => {
    const next = toggled(type, listOf(type, values), id);
    if (next === null) return;
    useSettingsStore.getState().set(settingKeyOf(type), next);
    setChanged(true);
  };

  const poster = artworkType("movie.poster");
  const fanart = artworkType("movie.fanart");
  const example = [poster, fanart]
    .flatMap((type) => (type === undefined ? [] : examplesOf(type, listOf(type, values))))
    .join(", ");

  return (
    <>
      <FormRow
        label={t("settings.metadata.artwork-names.title")}
        hint={t("settings.metadata.artwork-names.description")}
        labelId="naming-artwork-names"
        align="start"
      >
        <div className="flex flex-col gap-1.5" data-setting={ARTWORK_STYLE_KEY}>
          <Select
            className="w-[240px] max-w-full"
            aria-labelledby="naming-artwork-names"
            disabled={!ready}
            options={styles.map((value) => ({ value, label: styleLabel(value) }))}
            value={current}
            onValueChange={choose}
          />
          {custom ? (
            <div className="text-(length:--fs-12) text-text-muted">
              {t("settingsPage.artworkNames.styleNote")}
            </div>
          ) : (
            <div className="text-(length:--fs-12) text-text-2">
              {t("settingsPage.naming.example")}{" "}
              <span className="cv-selectable font-mono text-text [overflow-wrap:anywhere]">
                {example}
              </span>
            </div>
          )}
          {changed ? (
            <div role="status" className="text-(length:--fs-12) text-text-2">
              {t("settingsPage.artworkNames.renameHint")}
            </div>
          ) : null}
        </div>
      </FormRow>
      {custom ? (
        <div className={ROW_BLOCK} data-testid="artwork-names-table">
          <TypeTable scope="movie" values={values} onChange={change} />
          <TypeTable scope="tv" values={values} onChange={change} />
          <p className="m-0 text-(length:--fs-12) text-text-muted">
            {t("settingsPage.artworkNames.sharedNote")}
          </p>
        </div>
      ) : null}
    </>
  );
}
