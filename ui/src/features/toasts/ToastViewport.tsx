import { useCallback, useMemo } from "react";

import {
  TOAST_DURATION_MS,
  Toast,
  ToastStack,
  UNDO_TOAST_DURATION_MS,
} from "../../components/Toast";
import { selectBottomInset, useBottomInsetStore } from "../../stores/bottomInset";
import type { Platform } from "../shell/platform";
import { undoShortcutLabel } from "./useUndoShortcut";
import { useToastStore } from "./toastStore";
import type { ToastEntry } from "./toastStore";

interface ToastItemProps {
  entry: ToastEntry;
  shortcut: string;
}

function ToastItem({ entry, shortcut }: ToastItemProps) {
  const { id, message, count, tone } = entry;
  // Undo gets about 8 seconds; anything else is shorter. An explicit duration wins.
  const duration =
    entry.duration !== undefined
      ? entry.duration
      : entry.undo === undefined
        ? TOAST_DURATION_MS
        : UNDO_TOAST_DURATION_MS;
  const hasUndo = entry.undo !== undefined;
  const dismiss = useToastStore((state) => state.dismiss);
  const undo = useToastStore((state) => state.undo);
  // Stable callbacks keep the toast's timer from restarting on every render of the stack.
  const onDismiss = useCallback(() => dismiss(id), [dismiss, id]);
  const onUndo = useCallback(() => void undo(id), [undo, id]);
  const own = entry.action;
  const action = useMemo(
    () =>
      own === undefined
        ? undefined
        : {
            label: own.label,
            onClick: () => {
              own.onClick();
              dismiss(id);
            },
          },
    [dismiss, id, own],
  );
  return (
    <Toast
      message={message}
      glyph={entry.glyph}
      count={count}
      tone={tone}
      action={action}
      duration={duration}
      onDismiss={onDismiss}
      onUndo={hasUndo ? onUndo : undefined}
      undoShortcut={shortcut}
      className="pointer-events-auto"
    />
  );
}

interface ToastViewportProps {
  platform: Platform;
}

/**
 * The toast stack at the bottom right of the window, newest lowest, at most three; it rises above
 * the selection bar or any other bar along the bottom (spec `desktop-interaction`).
 */
export function ToastViewport({ platform }: ToastViewportProps) {
  const toasts = useToastStore((state) => state.toasts);
  const inset = useBottomInsetStore(selectBottomInset);
  const shortcut = undoShortcutLabel(platform);
  return (
    <ToastStack inset={inset}>
      {toasts.map((entry) => (
        <ToastItem key={entry.id} entry={entry} shortcut={shortcut} />
      ))}
    </ToastStack>
  );
}
