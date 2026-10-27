import { Check, Folder, Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { ReactNode, Ref } from "react";

import type { OrganizeProfileDto } from "../../bindings";
import { Button } from "../../components/Button";
import { IconButton } from "../../components/IconButton";
import { Icon } from "../../components/Icon";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { KeyField } from "../providers/KeyField";
import type { ProviderCredentials } from "../providers/providersBackend";
import type { ProviderKeys } from "../providers/useProviderKeys";
import type { SourceKind } from "../library-sources/sourcesBackend";
import { overlaps } from "./folderEntries";
import type { FolderEntry } from "./folderEntries";
import { currentOnboardingBackend } from "./onboardingBackend";
import type { NameExample } from "./onboardingBackend";

export type Step = 1 | 2 | 3 | 4;

const STEPS: readonly Step[] = [1, 2, 3, 4];

const STEP_LABELS: Record<Step, MessageKey> = {
  1: "setup.step.folders",
  2: "setup.step.tmdb",
  3: "setup.step.ratings",
  4: "setup.step.naming",
};

/** The numbered steps on the window ground: the current one in gold. */
export function Stepper({ step }: { step: Step }) {
  return (
    <ol aria-label={t("setup.steps")} className="m-0 flex list-none items-center gap-3.5 p-0">
      {STEPS.map((n, index) => {
        const current = n === step;
        const done = n < step;
        return (
          <li key={n} aria-current={current ? "step" : undefined} className="contents">
            {index > 0 ? <span aria-hidden="true" className="h-px flex-1 bg-line-edge" /> : null}
            <span
              className={cx(
                "flex items-center gap-2 text-(length:--fs-13)",
                current ? "font-(weight:--fw-600) text-text" : "text-text-muted",
              )}
            >
              <span
                aria-hidden="true"
                className={cx(
                  "flex size-5 items-center justify-center rounded-(--r-pill) border border-solid text-(length:--fs-11) font-(weight:--fw-600)",
                  current ? "border-gold bg-gold text-on-gold" : "border-line-kbd",
                )}
              >
                {done ? <Icon icon={Check} size={12} /> : n}
              </span>
              {t(STEP_LABELS[n])}
              {done ? <span className="sr-only">{t("setup.step.done")}</span> : null}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

interface StepHeadingProps {
  title: string;
  children: ReactNode;
  headingRef?: Ref<HTMLHeadingElement>;
}

/** The title and the sentence under it, at the top of the card of every step. */
function StepHeading({ title, children, headingRef }: StepHeadingProps) {
  return (
    <div className="px-6 pt-[22px] pb-4">
      <h1
        ref={headingRef}
        tabIndex={-1}
        className="m-0 mb-1 text-(length:--fs-16) font-(weight:--fw-700) outline-none"
      >
        {title}
      </h1>
      <p className="m-0 max-w-[75ch] text-text-2">{children}</p>
    </div>
  );
}

interface FolderRowProps {
  entry: FolderEntry;
  inside: string | null;
  onKind: (kind: SourceKind) => void;
  onRemove: () => void;
}

function FolderRow({ entry, inside, onKind, onRemove }: FolderRowProps) {
  const found = entry.found;
  let line: string;
  if (entry.status === "looking") line = t("setup.folders.looking");
  else if (found === null) line = entry.error ?? "";
  else {
    line = t(found.truncated ? "setup.folders.foundMore" : "setup.folders.found", {
      count: found.videos,
    });
    if (found.suggested === "tv") line += ` · ${t("setup.folders.looksTv")}`;
    else if (found.suggested === "undecided") line += ` · ${t("setup.folders.both")}`;
  }
  return (
    <li
      data-folder={entry.path}
      className="flex items-center gap-3 border-t border-solid border-line-row px-[14px] py-3"
    >
      <Icon icon={Folder} className="shrink-0 text-text-muted" />
      <div className="min-w-0 flex-1">
        <div className="cv-selectable truncate font-mono text-(length:--fs-13)" title={entry.path}>
          {entry.path}
        </div>
        <div
          className={cx(
            "text-(length:--fs-12)",
            entry.status === "failed" || inside !== null ? "text-danger" : "text-text-muted",
          )}
        >
          {inside !== null ? t("setup.folders.inside", { other: inside }) : line}
        </div>
      </div>
      {entry.status === "ready" ? (
        <SegmentedControl<SourceKind>
          aria-label={t("setup.folders.holds", { path: entry.path })}
          value={entry.kind}
          options={[
            { value: "movie", label: t("setup.folders.movies") },
            { value: "tv", label: t("setup.folders.tv") },
          ]}
          onChange={onKind}
        />
      ) : null}
      <IconButton
        icon={X}
        aria-label={t("setup.folders.remove", { path: entry.path })}
        onClick={onRemove}
      />
    </li>
  );
}

interface FoldersStepProps {
  entries: readonly FolderEntry[];
  over: boolean;
  headingRef: Ref<HTMLHeadingElement>;
  onBrowse: () => void;
  onKind: (path: string, kind: SourceKind) => void;
  onRemove: (path: string) => void;
}

/** Step 1: each folder with what was found and a Movies | TV shows switch, and the drop zone. */
export function FoldersStep({
  entries,
  over,
  headingRef,
  onBrowse,
  onKind,
  onRemove,
}: FoldersStepProps) {
  const clash = overlaps(entries);
  return (
    <>
      <StepHeading title={t("setup.folders.title")} headingRef={headingRef}>
        {t("setup.folders.intro")}
      </StepHeading>
      <ul aria-label={t("setup.folders.list")} className="m-0 list-none p-0">
        {entries.map((entry) => (
          <FolderRow
            key={entry.path}
            entry={entry}
            inside={
              clash.get(entry.path)?.inside === true ? (clash.get(entry.path)?.other ?? null) : null
            }
            onKind={(kind) => {
              onKind(entry.path, kind);
            }}
            onRemove={() => {
              onRemove(entry.path);
            }}
          />
        ))}
      </ul>
      <div
        data-drop={over ? "over" : undefined}
        className={cx(
          "mx-[14px] mt-1.5 mb-4 flex h-16 items-center justify-center gap-2.5 rounded-(--r-10) border-[1.5px] border-dashed text-text-2",
          over ? "border-gold bg-gold-tint" : "border-line-kbd",
        )}
      >
        <Button icon={Plus} onClick={onBrowse}>
          {t("setup.folders.add")}
        </Button>
        <span className="text-text-muted">{t("setup.folders.drop")}</span>
      </div>
    </>
  );
}

function find(keys: ProviderKeys, id: string): ProviderCredentials | undefined {
  return keys.providers.find((provider) => provider.id === id);
}

/** Step 2: the TMDB key, where to get it for free, and Test. */
export function TmdbStep({
  keys,
  headingRef,
}: {
  keys: ProviderKeys;
  headingRef: Ref<HTMLHeadingElement>;
}) {
  const provider = find(keys, "tmdb");
  return (
    <>
      <StepHeading title={t("setup.tmdb.title")} headingRef={headingRef}>
        {t("setup.tmdb.intro")}
      </StepHeading>
      <div className="flex flex-col gap-3 border-t border-solid border-line-row px-6 py-4">
        <KeyField provider={provider} keys={keys} label={t("settingsPage.providers.tmdb")} />
        <ol className="m-0 flex list-decimal flex-col gap-1 pl-5 text-text-2">
          <li>
            {t("setup.tmdb.step1")}{" "}
            {provider === undefined ? null : (
              <a
                href={provider.registration_url}
                target="_blank"
                rel="noreferrer noopener"
                className="text-text hover:underline"
              >
                {t("settingsPage.providers.getKey")}
              </a>
            )}
          </li>
          <li>{t("setup.tmdb.step2")}</li>
          <li>{t("setup.tmdb.step3")}</li>
        </ol>
      </div>
    </>
  );
}

/** Step 3: the optional OMDb and MDBList keys, with what each adds. */
export function RatingsStep({
  keys,
  headingRef,
}: {
  keys: ProviderKeys;
  headingRef: Ref<HTMLHeadingElement>;
}) {
  const rows = [
    { id: "omdb", name: "OMDb", label: "settingsPage.providers.omdb", adds: "setup.ratings.omdb" },
    {
      id: "mdblist",
      name: "MDBList",
      label: "settingsPage.providers.mdblist",
      adds: "setup.ratings.mdblist",
    },
  ] as const;
  return (
    <>
      <StepHeading title={t("setup.ratings.title")} headingRef={headingRef}>
        {t("setup.ratings.intro")}
      </StepHeading>
      {rows.map((row) => {
        const provider = find(keys, row.id);
        return (
          <div
            key={row.id}
            className="flex flex-col gap-1.5 border-t border-solid border-line-row px-6 py-3.5"
          >
            <div className="flex items-baseline gap-2">
              <span className="font-(weight:--fw-600)">{row.name}</span>
              <Tag>{t("setup.ratings.optional")}</Tag>
              <span className="text-(length:--fs-12) text-text-muted">{t(row.adds)}</span>
              {provider === undefined ? null : (
                <a
                  href={provider.registration_url}
                  target="_blank"
                  rel="noreferrer noopener"
                  className="ml-auto text-(length:--fs-12) text-text-2 hover:text-text hover:underline"
                >
                  {t("settingsPage.providers.getKey")}
                </a>
              )}
            </div>
            <KeyField provider={provider} keys={keys} label={t(row.label)} />
          </div>
        );
      })}
    </>
  );
}

interface NamingStepProps {
  profiles: readonly OrganizeProfileDto[];
  selected: string;
  onSelect: (id: string) => void;
  /** The first video the folder scan found; `null` while it found none (the sample is used). */
  fileName: string | null;
  headingRef: Ref<HTMLHeadingElement>;
}

/** Step 4: the naming styles, each with its result for one file, and the live example. */
export function NamingStep({
  profiles,
  selected,
  onSelect,
  fileName,
  headingRef,
}: NamingStepProps) {
  // The rendering of every style for one file, stored under the file it was made for, so an
  // older answer never shows for a newer file.
  const [examples, setExamples] = useState<Record<string, NameExample | "failed">>({});
  const keyOf = (id: string) => `${fileName ?? ""}::${id}`;
  const ids = profiles.map((profile) => profile.id).join("|");

  useEffect(() => {
    let live = true;
    for (const id of ids.split("|").filter((value) => value !== "")) {
      const key = `${fileName ?? ""}::${id}`;
      currentOnboardingBackend()
        .nameExample(id, fileName)
        .then((example) => {
          if (live) setExamples((current) => ({ ...current, [key]: example }));
        })
        .catch(() => {
          if (live) setExamples((current) => ({ ...current, [key]: "failed" }));
        });
    }
    return () => {
      live = false;
    };
  }, [ids, fileName]);

  const shown = examples[keyOf(selected)];
  return (
    <>
      <StepHeading title={t("setup.naming.title")} headingRef={headingRef}>
        {t("setup.naming.intro")}
      </StepHeading>
      <div
        role="group"
        aria-label={t("setup.naming.styles")}
        className="grid grid-cols-3 gap-2.5 px-[14px] pb-3"
      >
        {profiles.map((profile) => {
          const on = profile.id === selected;
          const example = examples[keyOf(profile.id)];
          return (
            <button
              key={profile.id}
              type="button"
              aria-pressed={on}
              onClick={() => {
                onSelect(profile.id);
              }}
              className={cx(
                "cv-hoverable flex min-w-0 flex-col gap-1 rounded-(--r-10) border border-solid p-3 text-left",
                on
                  ? "border-gold bg-surface-active"
                  : "border-line-edge bg-transparent hover:bg-surface-raised active:bg-surface-pressed",
              )}
            >
              <span className="font-(weight:--fw-600)">{profile.name}</span>
              <span className="text-(length:--fs-12) text-text-muted">
                {t(`setup.naming.style.${profile.builtin ? profile.id : "custom"}` as MessageKey)}
              </span>
              <span className="mt-1 min-h-5 font-mono text-(length:--fs-12) break-words text-text-2">
                {typeof example === "object" ? example.folder : ""}
              </span>
            </button>
          );
        })}
      </div>
      <div
        aria-live="polite"
        className="mx-[14px] mb-4 flex flex-col gap-1.5 rounded-(--r-10) border border-solid border-line-row bg-surface-well px-3.5 py-3"
      >
        <div className="flex items-center gap-2">
          <span className="font-(weight:--fw-600)">{t("setup.naming.example")}</span>
          {typeof shown === "object" ? (
            <Tag variant={shown.sample ? "neutral" : "ok"}>
              {t(shown.sample ? "setup.naming.sample" : "setup.naming.yours")}
            </Tag>
          ) : null}
        </div>
        {shown === "failed" ? (
          <p role="alert" className="m-0 text-danger">
            {t("setup.naming.failed")}
          </p>
        ) : shown === undefined ? (
          <p role="status" className="m-0 text-text-muted">
            {t("setup.naming.loading")}
          </p>
        ) : (
          <dl className="m-0 grid grid-cols-[max-content_1fr] items-baseline gap-x-4 gap-y-1">
            <dt className="text-text-muted">{t("setup.naming.now")}</dt>
            <dd className="cv-selectable m-0 font-mono text-text-2 [overflow-wrap:anywhere]">
              {shown.current}
            </dd>
            <dt className="text-text-muted">{t("setup.naming.folder")}</dt>
            <dd className="cv-selectable m-0 font-mono [overflow-wrap:anywhere]">{shown.folder}</dd>
            <dt className="text-text-muted">{t("setup.naming.file")}</dt>
            <dd className="cv-selectable m-0 font-mono [overflow-wrap:anywhere]">{shown.file}</dd>
          </dl>
        )}
        <p className="m-0 text-(length:--fs-12) text-text-muted">{t("setup.naming.nothing")}</p>
      </div>
    </>
  );
}
