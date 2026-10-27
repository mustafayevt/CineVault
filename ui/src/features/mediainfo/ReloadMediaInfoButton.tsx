import { FileSearch } from "lucide-react";
import { useState } from "react";

import { Button } from "../../components/Button";
import { IconButton } from "../../components/IconButton";
import { Popover } from "../../components/Popover";
import { t } from "../../i18n/catalog";
import { reloadMediaInfo, useReloadStore } from "./reloadMediaInfo";
import { useReloadScope } from "./reloadScope";

/**
 * The Library toolbar's Reload media info button, beside Scan folders. Scan only looks at files
 * that changed; this one reads the file details again. With titles selected it reloads those; with
 * none it reloads every title of the current list, after a quick confirmation under the button (a
 * popover, not a dialog), because that reads every file again and can take a few minutes.
 */
export function ReloadMediaInfoButton() {
  const { type, selected, all } = useReloadScope();
  const running = useReloadStore((state) => state.running);
  const [asking, setAsking] = useState(false);

  if (type === null) return null;
  const everything = selected.length === 0;
  const disabled = running || all.length === 0;

  return (
    <Popover
      open={asking && everything}
      onOpenChange={(open) => {
        if (!open) {
          setAsking(false);
        } else if (!everything) {
          // Titles are selected: no question, they are reloaded.
          void reloadMediaInfo(selected);
        } else {
          setAsking(true);
        }
      }}
      align="end"
      aria-label={t("library.reloadMedia.label")}
      className="w-[320px]"
      trigger={
        <IconButton
          icon={FileSearch}
          aria-label={t("library.reloadMedia.label")}
          disabled={disabled}
        />
      }
    >
      <p className="m-0 text-(length:--fs-13) text-text">
        {t(
          type === "movies"
            ? "library.reloadMedia.confirm.movies"
            : "library.reloadMedia.confirm.shows",
          {
            count: all.length,
          },
        )}
      </p>
      <div className="mt-3 flex justify-end gap-2">
        <Button size="sm" onClick={() => setAsking(false)}>
          {t("library.reloadMedia.cancel")}
        </Button>
        <Button
          size="sm"
          variant="primary"
          onClick={() => {
            setAsking(false);
            void reloadMediaInfo(all);
          }}
        >
          {t("library.reloadMedia.go")}
        </Button>
      </div>
    </Popover>
  );
}
