import { useEffect, useRef } from "react";

import { Button } from "../../components/Button";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { detectPlatform } from "../shell/platform";
import type { EditSession } from "./useItemEdit";

const platform = detectPlatform();

/**
 * The foot bar of Edit details: while there are unsaved changes it counts them and offers Discard
 * and Save (Ctrl+S); when the user tries to leave with changes the same bar asks what to do with
 * them, so nothing is lost silently and no dialog opens. A docked commit bar: card colour, a
 * section line above it, no shadow.
 */
export function EditFootBar({
  session,
  onSave,
  className,
}: {
  session: EditSession;
  onSave: () => void;
  className?: string;
}) {
  const { dirtyKeys, guard, saving, errors } = session;
  const keep = useRef<HTMLButtonElement>(null);
  const { blocked, stay } = guard;
  const stayNow = useRef(stay);
  useEffect(() => {
    stayNow.current = stay;
  });

  // The question takes the focus; Escape means Keep editing and gives the focus back.
  useEffect(() => {
    if (!blocked) return;
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    keep.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      event.stopPropagation();
      stayNow.current();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      if (before?.isConnected === true) before.focus();
    };
  }, [blocked]);

  if (dirtyKeys.length === 0 && !blocked) return null;

  const problems = Object.keys(errors).length;
  const saveAndLeave = async () => {
    if (await session.save()) guard.proceed();
  };
  const discardAndLeave = () => {
    session.discard();
    guard.proceed();
  };

  return (
    <section
      aria-label={t("edit.bar.label")}
      data-testid="edit-foot-bar"
      className={cx(
        "z-30 flex h-14 shrink-0 items-center gap-4 border-t border-solid border-line-section bg-surface-card px-(--inset)",
        blocked && "border-warn",
        className,
      )}
    >
      <p
        role="status"
        className="m-0 flex items-center gap-3 text-(length:--fs-13) font-(weight:--fw-600)"
      >
        <span aria-hidden="true" className="size-2 rounded-(--r-pill) bg-gold" />
        <span>
          {blocked
            ? t("edit.bar.leave", { count: dirtyKeys.length })
            : t("edit.bar.count", { count: dirtyKeys.length })}
        </span>
        {problems > 0 && !blocked ? (
          <span className="font-(weight:--fw-500) text-danger">
            {t("edit.bar.problems", { count: problems })}
          </span>
        ) : null}
      </p>
      <span className="flex-1" />
      {blocked ? (
        <>
          <Button ref={keep} variant="ghost" onClick={guard.stay}>
            {t("edit.bar.keepEditing")}
          </Button>
          <Button variant="danger" disabled={saving} onClick={discardAndLeave}>
            {t("edit.bar.discardLeave")}
          </Button>
          <Button variant="primary" disabled={saving} onClick={() => void saveAndLeave()}>
            {t("edit.bar.saveLeave")}
          </Button>
        </>
      ) : (
        <>
          <Button
            variant="ghost"
            disabled={saving}
            onClick={() => {
              session.discard();
              session.start();
            }}
          >
            {t("edit.bar.discard")}
          </Button>
          <Button
            variant="primary"
            disabled={saving}
            onClick={onSave}
            shortcut={platform === "macos" ? "⌘S" : "Ctrl S"}
          >
            {saving ? t("edit.bar.saving") : t("edit.bar.save")}
          </Button>
        </>
      )}
    </section>
  );
}
