import { Copy } from "lucide-react";

import { Button } from "../../components/Button";
import { Card } from "../../components/Card";
import { SectionLabel } from "../../components/SectionLabel";
import { t } from "../../i18n/catalog";
import type { NfoPreview } from "../detail/detailBackend";
import { copyText } from "../disclosure/copy";

interface NfoPanelProps {
  id: string;
  open: boolean;
  /** `undefined` while the NFO is being prepared. */
  nfo: NfoPreview | undefined;
  error: Error | null;
  onHide: () => void;
  onRewrite: () => void;
}

/**
 * The NFO text of a title in place under the Technical card (the ⋯ menu's Show NFO and the NFO
 * row's link open it): the text a Rewrite NFO would write, in mono, with Copy and Hide. When the
 * file on disk says something else, the panel says so and offers Rewrite NFO.
 */
export function NfoPanel({ id, open, nfo, error, onHide, onRewrite }: NfoPanelProps) {
  return (
    <div id={id} className="cv-expand" data-open={open ? "true" : "false"}>
      <div>
        <Card className="mt-4" aria-label={t("detail.nfo.text")}>
          <SectionLabel
            as="h3"
            action={
              <button type="button" onClick={onHide} className="cv-link">
                {t("movie.nfo.hide")}
              </button>
            }
          >
            {t("movie.nfo.title")}
          </SectionLabel>
          {error !== null ? (
            <p role="alert" className="m-0 text-warn">
              {t("detail.nfo.error", { message: error.message })}
            </p>
          ) : nfo === undefined ? (
            <p role="status" className="m-0 text-text-muted">
              {t("detail.nfo.loading")}
            </p>
          ) : (
            <div className="flex flex-col gap-3">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-(length:--fs-12) text-text-muted">
                <span>{t("detail.nfo.format", { format: nfo.format })}</span>
                {nfo.sync === "differs" ? (
                  <>
                    <span className="text-warn">{t("detail.nfo.sync.differs")}</span>
                    <Button size="sm" onClick={onRewrite}>
                      {t("detail.nfo.rewrite")}
                    </Button>
                  </>
                ) : null}
                <Button
                  size="sm"
                  variant="ghost"
                  icon={Copy}
                  className="ml-auto"
                  onClick={() =>
                    void copyText(nfo.text, t("detail.nfo.copied"), t("detail.nfo.copyFailed"))
                  }
                >
                  {t("detail.nfo.copy")}
                </Button>
              </div>
              {/* A scrollable region takes focus so the keyboard can scroll it. */}
              {/* eslint-disable jsx-a11y/no-noninteractive-tabindex */}
              <pre
                tabIndex={0}
                className="cv-selectable type-code m-0 max-h-[420px] overflow-auto rounded-(--r-6) bg-surface-well p-3 whitespace-pre-wrap text-text-2"
              >
                {nfo.text}
              </pre>
              {/* eslint-enable jsx-a11y/no-noninteractive-tabindex */}
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}
