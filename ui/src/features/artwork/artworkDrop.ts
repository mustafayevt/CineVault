import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { DragEvent } from "react";

import type { LocalImage } from "./artworkBackend";

/**
 * An element that takes dropped picture files carries these attributes. The attributes are how a
 * drop in the desktop window (which reports only a position, never a DOM event) finds out which
 * item and slot it landed on.
 */
const DROP_ATTRIBUTE = "data-art-drop";

export interface DropSlot {
  itemId: string;
  slot: string;
}

/** Spread on an element to make it a drop target for `slot` of `itemId`. */
export function dropAttributes(drop: DropSlot): Record<string, string> {
  return { [DROP_ATTRIBUTE]: "", "data-art-item": drop.itemId, "data-art-slot": drop.slot };
}

/** The slot a drop at `element` is meant for: the nearest drop target around it. */
function dropSlotOf(element: Element | null): DropSlot | null {
  const host = element?.closest(`[${DROP_ATTRIBUTE}]`);
  if (host === null || host === undefined) return null;
  const itemId = host.getAttribute("data-art-item");
  const slot = host.getAttribute("data-art-slot");
  return itemId === null || slot === null ? null : { itemId, slot };
}

function carriesFiles(event: DragEvent): boolean {
  return Array.from(event.dataTransfer.types).includes("Files");
}

/**
 * Drop handlers for an element, for the plain browser where pictures arrive as `File`s. `over` is
 * true while a file is dragged across the element, for its highlight. The desktop window never
 * fires these events; see {@link useNativeFileDrop}.
 */
export function useFileDrop(onFiles: (images: LocalImage[]) => void) {
  const [over, setOver] = useState(false);
  return {
    over,
    handlers: {
      onDragEnter: (event: DragEvent) => {
        if (carriesFiles(event)) setOver(true);
      },
      onDragOver: (event: DragEvent) => {
        if (!carriesFiles(event)) return;
        // Without this the browser opens the file instead of dropping it here.
        event.preventDefault();
        event.dataTransfer.dropEffect = "copy";
        setOver(true);
      },
      onDragLeave: (event: DragEvent) => {
        if (
          !(event.relatedTarget instanceof Node) ||
          !event.currentTarget.contains(event.relatedTarget)
        ) {
          setOver(false);
        }
      },
      onDrop: (event: DragEvent) => {
        setOver(false);
        if (!carriesFiles(event)) return;
        event.preventDefault();
        event.stopPropagation();
        const files = Array.from(event.dataTransfer.files);
        if (files.length > 0) onFiles(files.map((file) => ({ kind: "file", file })));
      },
    },
  };
}

/** Marks the drop target under the pointer while a file is dragged over the window. */
const OVER_ATTRIBUTE = "data-art-over";

function clearOver(): void {
  for (const element of document.querySelectorAll(`[${OVER_ATTRIBUTE}]`)) {
    element.removeAttribute(OVER_ATTRIBUTE);
  }
}

function targetAt(x: number, y: number): Element | null {
  const ratio = window.devicePixelRatio > 0 ? window.devicePixelRatio : 1;
  return document.elementFromPoint(x / ratio, y / ratio);
}

/**
 * In the desktop window a file dropped from the file manager arrives as the window's drag-and-drop
 * event with paths and a position. This finds the drop target at that position, highlights it
 * while the file hovers, and hands the paths over on the drop. Mount it once, in the shell.
 */
export function useNativeFileDrop(onDrop: (drop: DropSlot, images: LocalImage[]) => void): void {
  useEffect(() => {
    if (!isTauri()) return;
    let stopped = false;
    let unlisten: (() => void) | null = null;
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          const payload = event.payload;
          if (payload.type === "leave") {
            clearOver();
            return;
          }
          const host = targetAt(payload.position.x, payload.position.y)?.closest(
            `[${DROP_ATTRIBUTE}]`,
          );
          clearOver();
          if (payload.type === "drop") {
            const drop = dropSlotOf(host ?? null);
            if (drop !== null && payload.paths.length > 0) {
              onDrop(
                drop,
                payload.paths.map((path) => ({ kind: "path", path })),
              );
            }
            return;
          }
          host?.setAttribute(OVER_ATTRIBUTE, "");
        }),
      )
      .then((stop) => {
        if (stopped) stop();
        else unlisten = stop;
      })
      .catch(() => undefined);
    return () => {
      stopped = true;
      unlisten?.();
      clearOver();
    };
  }, [onDrop]);
}
