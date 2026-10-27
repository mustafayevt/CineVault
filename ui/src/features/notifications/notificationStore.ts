import { create } from "zustand";

export type NotificationSeverity = "info" | "warning" | "error";

export interface NotificationInput {
  severity: NotificationSeverity;
  /** One line: what happened. */
  title: string;
  /** More detail, such as the reason or the affected item. */
  detail?: string;
  /** Where it came from, for example a provider or job name. */
  source?: string;
  /** The provider whose key was rejected or rate limited: the message links to its card. */
  providerKey?: string;
  /** The fix the message offers, such as `retry_imdb_dataset`; Needs attention shows its button. */
  action?: string;
  /** The unfinished move or rename the message is about (with the `resolve_unfinished` action). */
  batchId?: string;
  /**
   * What the message is about (`source-skipped:<folder>`): a later message of the same topic
   * replaces it, and [`resolveTopic`] takes it away once it no longer holds.
   */
  topic?: string;
}

export interface AppNotification extends NotificationInput {
  id: string;
  /** Epoch milliseconds when it was added, or last repeated. */
  at: number;
  /** How many times the same message came in a row; absent for once. */
  repeats?: number;
}

function sameMessage(a: NotificationInput, b: NotificationInput): boolean {
  return (
    a.severity === b.severity &&
    a.title === b.title &&
    (a.detail ?? "") === (b.detail ?? "") &&
    a.source === b.source &&
    a.providerKey === b.providerKey &&
    a.action === b.action &&
    a.batchId === b.batchId
  );
}

/** The notification centre keeps this many messages; the oldest drop off first. */
export const MAX_NOTIFICATIONS = 200;

interface NotificationState {
  /**
   * What background work reported, newest first. Warnings and errors wait in Activity's Needs
   * attention until they are resolved or dismissed; information goes to History.
   */
  items: readonly AppNotification[];
  add: (input: NotificationInput) => string;
  dismiss: (id: string) => void;
  /** Drops the messages of a topic that no longer holds (a folder that reads again). */
  resolveTopic: (topic: string) => void;
  clear: () => void;
}

let counter = 0;

export const useNotificationStore = create<NotificationState>((set, get) => ({
  items: [],
  add: (input) => {
    // The same message again (a button pressed five times, also with other messages in between)
    // is merged into the earlier copy with a count and moves to the top, so Activity does not
    // fill with copies.
    const items = get().items;
    const earlier = items.find((item) => sameMessage(item, input));
    if (earlier !== undefined) {
      const merged: AppNotification = {
        ...earlier,
        at: Date.now(),
        repeats: (earlier.repeats ?? 1) + 1,
      };
      set((state) => ({
        items: [merged, ...state.items.filter((item) => item.id !== earlier.id)],
      }));
      return earlier.id;
    }
    counter += 1;
    const id = `n${counter}`;
    const entry: AppNotification = { ...input, id, at: Date.now() };
    // A message about the same thing as an earlier one (a folder under its new label, or now
    // empty instead of unreadable) replaces it.
    const topic = input.topic;
    set((state) => ({
      items: [
        entry,
        ...state.items.filter((item) => topic === undefined || item.topic !== topic),
      ].slice(0, MAX_NOTIFICATIONS),
    }));
    return id;
  },
  dismiss: (id) => set((state) => ({ items: state.items.filter((item) => item.id !== id) })),
  resolveTopic: (topic) =>
    set((state) => ({ items: state.items.filter((item) => item.topic !== topic) })),
  clear: () => set({ items: [] }),
}));

/** Reports a message from background work to Activity. Errors go here, never to a dialog. */
export function notify(input: NotificationInput): string {
  return useNotificationStore.getState().add(input);
}
