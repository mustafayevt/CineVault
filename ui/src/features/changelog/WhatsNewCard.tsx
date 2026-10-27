import { useNavigate } from "@tanstack/react-router";
import { Sparkles, X } from "lucide-react";
import { useState } from "react";
import type { CSSProperties } from "react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { Presence } from "../../components/Presence";
import { t } from "../../i18n/catalog";
import { selectBottomInset, useBottomInsetStore } from "../../stores/bottomInset";
import { useEscapeLayer } from "../shell/escStack";
import { NotesBody } from "../updates/NotesBody";
import { parseNotes } from "../updates/notesFormat";
import { useWhatsNewStore } from "./whatsNew";

/**
 * The one-time card after an update: "Updated to CineVault 1.2.0 · What's new" with that
 * version's changes, at the bottom right above the toasts' corner. It floats on the floating
 * surface, slides in with the toast motion, closes with its button or Esc, and "See all changes"
 * opens the release notes page. Nothing is drawn when no update was just installed.
 */
export function WhatsNewCard() {
  const section = useWhatsNewStore((state) => state.section);
  const dismiss = useWhatsNewStore((state) => state.dismiss);
  const navigate = useNavigate();
  const inset = useBottomInsetStore(selectBottomInset);
  useEscapeLayer("menu", section !== null, dismiss);
  // The last section stays on the card while it slides out.
  const [shown, setShown] = useState(section);
  if (section !== null && section !== shown) setShown(section);
  return (
    <Presence present={section !== null}>
      {(presence) => (
        <WhatsNewBody
          presence={presence}
          version={shown?.version ?? ""}
          body={shown?.body ?? ""}
          inset={inset}
          onClose={dismiss}
          onSeeAll={() => {
            dismiss();
            void navigate({ to: "/release-notes" });
          }}
        />
      )}
    </Presence>
  );
}

interface BodyProps {
  presence: { "data-state": "open" | "closed" };
  version: string;
  body: string;
  inset: number;
  onClose: () => void;
  onSeeAll: () => void;
}

function WhatsNewBody({ presence, version, body, inset, onClose, onSeeAll }: BodyProps) {
  return (
    <aside
      {...presence}
      aria-labelledby="whats-new-title"
      data-testid="whats-new"
      style={
        {
          bottom: `calc(var(--space-4) + ${inset}px)`,
          "--cv-slide-x": "16px",
          "--cv-slide-y": "8px",
        } as CSSProperties
      }
      className="cv-slide-up fixed right-4 z-30 flex max-h-[min(460px,calc(100%-96px))] w-[380px] max-w-[calc(100%-32px)] flex-col overflow-hidden rounded-(--r-10) border border-solid border-line-edge bg-surface-floating text-(length:--fs-13) text-text shadow-floating"
    >
      <div className="flex items-start gap-2.5 py-3 pr-2 pl-4">
        <Icon icon={Sparkles} size={16} className="mt-0.5 shrink-0 text-gold" />
        <div className="min-w-0 flex-1">
          <h2 id="whats-new-title" className="m-0 text-(length:--fs-13) font-(weight:--fw-600)">
            {t("changelog.whatsNew.title", { version })}
          </h2>
          <p className="m-0 mt-px text-(length:--fs-12) text-text-muted">
            {t("changelog.whatsNew.subtitle")}
          </p>
        </div>
        <IconButton aria-label={t("common.close")} icon={X} size="sm" onClick={onClose} />
      </div>
      {/* A long list scrolls here, and can be scrolled with the keyboard. */}
      <div
        // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
        tabIndex={0}
        className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto border-t border-solid border-line-row px-4 py-3 text-text-2 outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-gold"
      >
        <NotesBody blocks={parseNotes(body)} />
      </div>
      <div className="flex items-center justify-end gap-1.5 border-t border-solid border-line-row px-2 py-2">
        <Button variant="ghost" size="sm" onClick={onSeeAll}>
          {t("changelog.whatsNew.seeAll")}
        </Button>
        <Button size="sm" onClick={onClose}>
          {t("common.close")}
        </Button>
      </div>
    </aside>
  );
}
