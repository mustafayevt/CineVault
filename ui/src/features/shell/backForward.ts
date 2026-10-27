import { useRouter } from "@tanstack/react-router";
import { useEffect } from "react";

/** The mouse's back and forward side buttons, as `MouseEvent.button` numbers them. */
const MOUSE_BACK = 3;
const MOUSE_FORWARD = 4;

function inTextField(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target instanceof HTMLTextAreaElement ||
    (target instanceof HTMLInputElement && !["checkbox", "radio", "button"].includes(target.type))
  );
}

/**
 * CineVault's own back and forward (spec `app-shell` "Back and forward"): Alt+Left and Alt+Right,
 * and the mouse's side buttons, move through the visited pages. The webview's own handling of
 * those keys and buttons is stopped, so a press never moves twice. In a text field Alt with an
 * arrow stays with the field. Mount once, in the shell.
 */
export function useBackForwardInput(): void {
  const router = useRouter();
  useEffect(() => {
    const history = router.history;
    const onKey = (event: KeyboardEvent) => {
      if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
      if (inTextField(event.target)) return;
      event.preventDefault();
      if (event.key === "ArrowLeft") history.back();
      else history.forward();
    };
    const swallow = (event: MouseEvent) => {
      if (event.button === MOUSE_BACK || event.button === MOUSE_FORWARD) event.preventDefault();
    };
    const onMouseUp = (event: MouseEvent) => {
      if (event.button !== MOUSE_BACK && event.button !== MOUSE_FORWARD) return;
      event.preventDefault();
      if (event.button === MOUSE_BACK) history.back();
      else history.forward();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", swallow);
    window.addEventListener("auxclick", swallow);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", swallow);
      window.removeEventListener("auxclick", swallow);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [router]);
}
