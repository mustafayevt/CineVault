import { create } from "zustand";

/** How long a title stays marked: the flash itself lasts `--dur-expand` (180ms). */
const HIGHLIGHT_MS = 400;

interface HighlightState {
  /** A counter per title that changed; it restarts the flash when a title changes again. */
  pulses: ReadonlyMap<string, number>;
  /** Highlights the titles briefly. */
  mark: (ids: readonly string[]) => void;
  clear: () => void;
}

const timers = new Map<string, ReturnType<typeof setTimeout>>();

export const useHighlightStore = create<HighlightState>((set, get) => ({
  pulses: new Map(),
  mark: (ids) => {
    if (ids.length === 0) return;
    const next = new Map(get().pulses);
    for (const id of ids) {
      next.set(id, (next.get(id) ?? 0) + 1);
      const earlier = timers.get(id);
      if (earlier !== undefined) clearTimeout(earlier);
      timers.set(
        id,
        setTimeout(() => {
          timers.delete(id);
          const rest = new Map(get().pulses);
          rest.delete(id);
          set({ pulses: rest });
        }, HIGHLIGHT_MS),
      );
    }
    set({ pulses: next });
  },
  clear: () => {
    for (const timer of timers.values()) clearTimeout(timer);
    timers.clear();
    set({ pulses: new Map() });
  },
}));
