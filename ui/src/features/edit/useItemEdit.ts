import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo, useState } from "react";

import { t } from "../../i18n/catalog";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { EditRequestError, currentEditBackend } from "./editBackend";
import type { EditField, EditTarget, EditValue, ItemEdit } from "./editBackend";
import { indexFields, isBlank, meaningful, problemOf, problemsOf, sameValue } from "./editModel";
import { useLeaveGuard } from "./useLeaveGuard";
import type { LeaveGuard } from "./useLeaveGuard";

export type { LeaveGuard } from "./useLeaveGuard";

/** Queries that show an item: they are read again after a save. */
const ITEM_QUERIES: ReadonlySet<unknown> = new Set([
  "movie-detail",
  "movie-nfo",
  "tv-show",
  "tv-episode",
  "tv-nfo",
  "library",
]);

/** One edit session of one item: the draft, its problems, the locks and the save. */
export interface EditSession {
  target: EditTarget;
  /** Something on the page shows its fields as controls: the whole page or any section. */
  editing: boolean;
  /** The sections being edited in place, by section id. */
  sections: readonly string[];
  /** Whether a section shows its fields as controls (it does while the whole page is edited). */
  isEditing: (section: string) => boolean;
  /** Starts editing one section in place; the rest of the page stays readable. */
  startSection: (section: string) => void;
  /** Leaves one section when nothing is unsaved. */
  stopSection: (section: string) => void;
  /**
   * Asks for the locks (and fields) to be read while the page is only read, so a locked field
   * can show its lock glyph. Returns the function that withdraws the request.
   */
  watchLocks: () => () => void;
  /** The fields are being read. */
  loading: boolean;
  loadError: string | null;
  /** Enters edit mode. */
  start: () => void;
  /**
   * What the page's Edit button and E do: edit the section of the open tab, or the whole page
   * where the page has no sections. Set by the `EditScope`.
   */
  startDefault?: () => void;
  /** Leaves edit mode when nothing is unsaved. */
  stop: () => void;
  /** The saved fields by key. */
  base: ReadonlyMap<string, EditField>;
  /** The value now on screen: the draft, otherwise the saved one. */
  get: (key: string) => EditValue | undefined;
  set: (key: string, next: EditValue) => void;
  /** Whether the field differs from what is saved. */
  isDirty: (key: string) => boolean;
  dirtyKeys: readonly string[];
  dirty: boolean;
  /** The reason the last save refused a field, by key. */
  errors: Readonly<Record<string, string>>;
  saving: boolean;
  /** Saves the changed fields; resolves to whether it worked. */
  save: () => Promise<boolean>;
  /** Drops every unsaved change and leaves edit mode. */
  discard: () => void;
  /** Lock keys of the locked fields. */
  locked: ReadonlySet<string>;
  itemLocked: boolean;
  /** The collections a movie can join, by name; empty for other kinds. */
  collections: readonly string[];
  toggleLock: (lockKey: string) => void;
  toggleItemLock: () => void;
  guard: LeaveGuard;
}

/**
 * Runs the edit session of the item on screen. `label` names the item in toasts. The fields are
 * read when edit mode starts, so a page that is only read never asks for them.
 */
export function useItemEdit(
  target: EditTarget,
  label: string,
  options: { startEditing?: boolean } = {},
): EditSession {
  const backend = currentEditBackend();
  const queryClient = useQueryClient();
  const [pageEditing, setPageEditing] = useState(options.startEditing === true);
  const [sections, setSections] = useState<readonly string[]>([]);
  const [watchers, setWatchers] = useState(0);
  const editing = pageEditing || sections.length > 0;
  const [draft, setDraft] = useState<Readonly<Record<string, EditValue>>>({});
  const [errors, setErrors] = useState<Readonly<Record<string, string>>>({});
  const [saving, setSaving] = useState(false);

  const queryKey = useMemo(() => ["item-edit", target.kind, target.id], [target.kind, target.id]);
  const query = useQuery({
    queryKey,
    queryFn: () => backend.load(target),
    enabled: editing || watchers > 0,
    staleTime: 0,
    gcTime: 0,
    refetchOnWindowFocus: false,
  });

  const base = useMemo(() => indexFields(query.data?.fields ?? []), [query.data]);
  const collections = useMemo(() => query.data?.collections ?? [], [query.data]);
  const dirtyKeys = useMemo(
    () =>
      Object.keys(draft).filter((key) => {
        const saved = base.get(key)?.value;
        const typed = draft[key];
        const next = typed === undefined ? undefined : meaningful(typed);
        // A field that was never stored reads as empty, so typing and clearing it is no change.
        return saved === undefined ? !isBlank(next) : !sameValue(next, saved);
      }),
    [draft, base],
  );
  const dirty = dirtyKeys.length > 0;

  const get = useCallback(
    (key: string): EditValue | undefined => draft[key] ?? base.get(key)?.value,
    [draft, base],
  );

  // A value that cannot be saved says so under its field as soon as it is typed.
  const set = useCallback((key: string, next: EditValue) => {
    setDraft((current) => ({ ...current, [key]: next }));
    const problem = problemOf(key, next);
    setErrors((current) => {
      const rest = Object.fromEntries(Object.entries(current).filter(([k]) => k !== key));
      if (problem !== null) return { ...rest, [key]: problem };
      return key in current ? rest : current;
    });
  }, []);

  const isDirty = useCallback((key: string) => dirtyKeys.includes(key), [dirtyKeys]);

  const refresh = useCallback(async () => {
    await queryClient.invalidateQueries({
      predicate: (q) => ITEM_QUERIES.has(q.queryKey[0]),
    });
  }, [queryClient]);

  // Fields read while the page was only being read (for the lock glyphs) may be old by now.
  const readAgain = useCallback(
    () => void queryClient.invalidateQueries({ queryKey, exact: true }),
    [queryClient, queryKey],
  );

  const start = useCallback(() => {
    setErrors({});
    readAgain();
    setPageEditing(true);
  }, [readAgain]);

  const startSection = useCallback(
    (section: string) => {
      setErrors({});
      readAgain();
      setSections((current) => (current.includes(section) ? current : [...current, section]));
    },
    [readAgain],
  );

  const discard = useCallback(() => {
    setDraft({});
    setErrors({});
    setPageEditing(false);
    setSections([]);
  }, []);

  const stopSection = useCallback(
    (section: string) => {
      if (dirty) return;
      setSections((current) => current.filter((id) => id !== section));
    },
    [dirty],
  );

  const isEditing = useCallback(
    (section: string) => pageEditing || sections.includes(section),
    [pageEditing, sections],
  );

  const watchLocks = useCallback(() => {
    setWatchers((n) => n + 1);
    return () => setWatchers((n) => Math.max(0, n - 1));
  }, []);

  const stop = useCallback(() => {
    if (!dirty) discard();
  }, [dirty, discard]);

  const save = useCallback(async (): Promise<boolean> => {
    if (saving) return false;
    if (dirtyKeys.length === 0) {
      discard();
      return true;
    }
    const changes: EditField[] = dirtyKeys.flatMap((key) => {
      const next = draft[key];
      return next === undefined
        ? []
        : [{ key, value: meaningful(next), lock: base.get(key)?.lock ?? null }];
    });
    // Every field that cannot be saved is marked at once, and nothing is sent.
    const problems = problemsOf(Object.fromEntries(changes.map((c) => [c.key, c.value])));
    if (Object.keys(problems).length > 0) {
      setErrors(problems);
      return false;
    }
    setSaving(true);
    try {
      const saved = await backend.save(target, changes);
      setDraft({});
      setErrors({});
      setPageEditing(false);
      setSections([]);
      await refresh();
      if (saved.changed.length > 0) {
        showToast({
          message: t("edit.toast.saved", { title: label, count: saved.changed.length }),
          undo: async () => {
            await backend.save(target, saved.previous);
            await refresh();
          },
        });
      }
      if (saved.nfo_problem !== null) {
        notify({
          severity: "warning",
          title: t("edit.nfo.failed", { title: label }),
          detail: saved.nfo_problem,
        });
      }
      return true;
    } catch (error) {
      if (error instanceof EditRequestError && error.code === "invalid" && error.field !== null) {
        setErrors({ [error.field]: error.message });
      } else {
        notify({
          severity: "error",
          title: t("edit.save.failed", { title: label }),
          detail: error instanceof Error ? error.message : String(error),
        });
      }
      return false;
    } finally {
      setSaving(false);
    }
  }, [saving, dirtyKeys, draft, base, backend, target, label, refresh, discard]);

  const setLocks = useCallback(
    (update: (data: ItemEdit) => ItemEdit) => {
      queryClient.setQueryData<ItemEdit>(queryKey, (data) =>
        data === undefined ? data : update(data),
      );
    },
    [queryClient, queryKey],
  );

  const locked = useMemo(() => new Set(query.data?.locked_fields ?? []), [query.data]);
  const itemLocked = query.data?.item_locked ?? false;

  const toggleLock = useCallback(
    (lockKey: string) => {
      const next = !locked.has(lockKey);
      const apply = (on: boolean) =>
        setLocks((data) => ({
          ...data,
          locked_fields: on
            ? [...data.locked_fields.filter((k) => k !== lockKey), lockKey]
            : data.locked_fields.filter((k) => k !== lockKey),
        }));
      apply(next);
      backend.setFieldLock(target, lockKey, next).catch((error: unknown) => {
        apply(!next);
        notify({
          severity: "error",
          title: t("edit.lock.failed", { title: label }),
          detail: error instanceof Error ? error.message : String(error),
        });
      });
    },
    [locked, backend, target, label, setLocks],
  );

  const toggleItemLock = useCallback(() => {
    const next = !itemLocked;
    setLocks((data) => ({ ...data, item_locked: next }));
    backend.setItemLock(target, next).catch((error: unknown) => {
      setLocks((data) => ({ ...data, item_locked: !next }));
      notify({
        severity: "error",
        title: t("edit.lock.failed", { title: label }),
        detail: error instanceof Error ? error.message : String(error),
      });
    });
  }, [itemLocked, backend, target, label, setLocks]);

  // Leaving the page with unsaved changes asks first, inline in the dirty bar: nothing is lost
  // silently.
  const guard = useLeaveGuard(dirty);

  return {
    target,
    editing,
    loading: editing && query.isPending,
    sections,
    isEditing,
    startSection,
    stopSection,
    watchLocks,
    loadError: query.error === null ? null : query.error.message,
    start,
    stop,
    base,
    get,
    set,
    isDirty,
    dirtyKeys,
    dirty,
    errors,
    saving,
    save,
    discard,
    locked,
    itemLocked,
    collections,
    toggleLock,
    toggleItemLock,
    guard,
  };
}
