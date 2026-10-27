import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { OrganizePreviewDto, OrganizeProfilesDto, PatternsDto } from "../../bindings";
import { currentOrganizeBackend } from "../rename/organizeBackend";
import { RENAME_KEY } from "../rename/useRenamePlan";
import { displayProblem, toDisplay, toEngine, tokensOf } from "./namingModel";
import type { NamingField } from "./namingModel";

/** How long typing rests before the template is checked and saved. */
const SAVE_DELAY_MS = 350;

const PROFILES_KEY = [...RENAME_KEY, "profiles"] as const;

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

type Texts = Partial<Record<NamingField, string>>;

export interface NamingTemplates {
  ready: boolean;
  /** What the field shows now: the text being typed, else the saved template. */
  valueOf: (field: NamingField) => string;
  /** Why the text of the field is not saved, or `null`. */
  problemOf: (field: NamingField) => string | null;
  /** Changes a field. Typing rests for {@link SAVE_DELAY_MS}; `now` checks and saves at once. */
  change: (field: NamingField, text: string, now?: boolean) => void;
  /** The plan for the templates as they are being edited: its `examples` are the live examples. */
  preview: OrganizePreviewDto | undefined;
  /** The naming styles: the presets (shared with the first run's Naming step) and the user's own. */
  styles: readonly NamingStyle[];
  /** The style in use, or `undefined` while the profiles load. */
  activeStyle: NamingStyle | undefined;
  /**
   * Makes a style the active one. `discard` is the custom style it replaces, which is deleted
   * (the caller has asked first). Resolves to why it failed, or `null`.
   */
  choose: (id: string, discard?: string) => Promise<string | null>;
}

/** A naming style as the Style control lists it. */
export interface NamingStyle {
  id: string;
  name: string;
  /** A preset; the others are the user's own ("Custom"). */
  builtin: boolean;
}

/**
 * The templates of the Naming section (spec `renamer` "Naming templates in Settings"). They are
 * those of the active naming profile; the first change copies a preset into a profile of the
 * user's own ("Custom"), later ones change that profile. A template is saved only when its braces
 * are closed and the engine accepts it, so the last valid template stays in use while an invalid
 * one is on screen. The live examples come from the plan for the templates being edited.
 */
export function useNamingTemplates(): NamingTemplates {
  const backend = currentOrganizeBackend();
  const client = useQueryClient();
  const profiles = useQuery({
    queryKey: PROFILES_KEY,
    queryFn: () => backend.profiles(),
    staleTime: 0,
  });
  const active =
    profiles.data?.profiles.find((profile) => profile.id === profiles.data?.active) ??
    profiles.data?.profiles[0];
  const activeRef = useRef(active);
  useEffect(() => {
    activeRef.current = active;
  }, [active]);

  const [texts, setTexts] = useState<Texts>({});
  const [problems, setProblems] = useState<Texts>({});
  // The templates the examples are drawn from: the saved ones plus the valid texts being edited.
  const [edited, setEdited] = useState<Partial<PatternsDto>>({});
  const timers = useRef<Partial<Record<NamingField, number>>>({});
  const saving = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const timer of Object.values(pending)) window.clearTimeout(timer);
    };
  }, []);

  const setProblem = useCallback((field: NamingField, message: string | null) => {
    setProblems((current) => {
      return message === null ? omit(current, field) : { ...current, [field]: message };
    });
  }, []);

  const save = useCallback(
    async (field: NamingField, engine: string) => {
      const profile = activeRef.current;
      if (profile === undefined) return;
      if (profile.patterns[field] === engine) {
        setTexts((current) => omit(current, field));
        setEdited((current) => omit(current, field));
        return;
      }
      const patterns: PatternsDto = { ...profile.patterns, [field]: engine };
      try {
        const saved: OrganizeProfilesDto = profile.builtin
          ? await backend.saveProfile("Custom", profile.id, patterns)
          : await backend.updateProfile(profile.id, patterns);
        client.setQueryData(PROFILES_KEY, saved);
        await client.invalidateQueries({ queryKey: RENAME_KEY });
        setTexts((current) => omit(current, field));
        setEdited((current) => omit(current, field));
      } catch (error) {
        setProblem(field, messageOf(error));
      }
    },
    [backend, client, setProblem],
  );

  const commit = useCallback(
    async (field: NamingField, text: string) => {
      const engine = toEngine(text, tokensOf(field));
      try {
        const findings = await backend.checkTemplate(field, engine);
        const refusal = findings.find((finding) => finding.severity === "error");
        if (refusal !== undefined) {
          setProblem(field, refusal.message);
          return;
        }
      } catch (error) {
        setProblem(field, messageOf(error));
        return;
      }
      setProblem(field, null);
      setEdited((current) => ({ ...current, [field]: engine }));
      // Saves run one after another, so the last change is the one that stays.
      saving.current = saving.current.then(() => save(field, engine));
      await saving.current;
    },
    [backend, save, setProblem],
  );

  const change = useCallback(
    (field: NamingField, text: string, now = false) => {
      setTexts((current) => ({ ...current, [field]: text }));
      window.clearTimeout(timers.current[field]);
      const refusal = displayProblem(text, tokensOf(field));
      setProblem(field, refusal);
      if (refusal !== null) return;
      if (now) void commit(field, text);
      else timers.current[field] = window.setTimeout(() => void commit(field, text), SAVE_DELAY_MS);
    },
    [commit, setProblem],
  );

  const choose = useCallback(
    async (id: string, discard?: string) => {
      // What was typed but not saved yet is dropped with the old style.
      for (const timer of Object.values(timers.current)) window.clearTimeout(timer);
      timers.current = {};
      try {
        await saving.current;
        let saved = await backend.selectProfile(id);
        if (discard !== undefined && discard !== id) saved = await backend.deleteProfile(discard);
        client.setQueryData(PROFILES_KEY, saved);
        setTexts({});
        setProblems({});
        setEdited({});
        await client.invalidateQueries({ queryKey: RENAME_KEY });
        return null;
      } catch (error) {
        return messageOf(error);
      }
    },
    [backend, client],
  );

  const request = useMemo(
    () =>
      active === undefined
        ? null
        : {
            scope: { kind: "everything" as const },
            profile_id: active.id,
            patterns: { ...active.patterns, ...edited },
            choices: [],
          },
    [active, edited],
  );
  const plan = useQuery({
    queryKey: [...RENAME_KEY, "plan", request],
    queryFn: () => {
      if (request === null) throw new Error("no naming profile");
      return backend.preview(request);
    },
    enabled: request !== null,
    staleTime: 0,
    placeholderData: keepPreviousData,
  });

  return {
    ready: active !== undefined,
    valueOf: (field) =>
      texts[field] ??
      (active === undefined ? "" : toDisplay(active.patterns[field], tokensOf(field))),
    problemOf: (field) => problems[field] ?? null,
    change,
    preview: plan.data,
    styles: profiles.data?.profiles ?? [],
    activeStyle: active,
    choose,
  };
}

function omit<T extends object>(value: T, key: keyof T): T {
  return Object.fromEntries(Object.entries(value).filter(([name]) => name !== key)) as T;
}
