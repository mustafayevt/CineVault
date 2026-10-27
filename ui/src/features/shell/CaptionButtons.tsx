import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import type { WindowApi } from "../shell/windowApi";

/** DOM id of the maximize button. Must match `MAXIMIZE_BUTTON_ID` in `src-tauri/src/chrome.rs`. */
export const MAXIMIZE_BUTTON_ID = "cv-caption-maximize";

/** 46 by 32, flush with the window's top right edge; hover and pressed climb the surface ladder. */
const button =
  "cv-hoverable inline-flex h-8 w-[46px] items-center justify-center bg-transparent text-text-2 " +
  "hover:bg-surface-raised hover:text-text active:bg-surface-active " +
  "[&.is-hovered]:bg-surface-raised [&.is-hovered]:text-text " +
  "focus-visible:-outline-offset-2";

const closeButton =
  "hover:bg-danger hover:text-on-gold active:bg-danger [&.is-hovered]:bg-danger [&.is-hovered]:text-on-gold";

function Glyph({ children }: { children: React.ReactNode }) {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 10 10"
      fill="none"
      stroke="currentColor"
      strokeWidth="1"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

interface CaptionButtonsProps {
  windowApi: WindowApi | null;
  maximized: boolean;
}

/**
 * Windows minimize, maximize/restore and close. The maximize button is the target of the native
 * snap-layout region (see `src-tauri/src/chrome.rs`); a mouse click there is handled natively, the
 * handler below serves keyboard activation.
 */
export function CaptionButtons({ windowApi, maximized }: CaptionButtonsProps) {
  return (
    <div className="flex h-8" role="group" aria-label={t("shell.window.controls")}>
      <button
        type="button"
        aria-label={t("shell.window.minimize")}
        className={button}
        onClick={() => void windowApi?.minimize()}
      >
        <Glyph>
          <path d="M0 5h10" />
        </Glyph>
      </button>
      <button
        type="button"
        id={MAXIMIZE_BUTTON_ID}
        aria-label={maximized ? t("shell.window.restore") : t("shell.window.maximize")}
        className={button}
        onClick={() => void windowApi?.toggleMaximize()}
      >
        <Glyph>
          {maximized ? (
            <>
              <rect x="0.5" y="2.5" width="7" height="7" />
              <path d="M2.5 2.5v-2h7v7h-2" />
            </>
          ) : (
            <rect x="0.5" y="0.5" width="9" height="9" rx="1.5" />
          )}
        </Glyph>
      </button>
      <button
        type="button"
        aria-label={t("common.close")}
        className={cx(button, closeButton)}
        onClick={() => void windowApi?.close()}
      >
        <Glyph>
          <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />
        </Glyph>
      </button>
    </div>
  );
}
