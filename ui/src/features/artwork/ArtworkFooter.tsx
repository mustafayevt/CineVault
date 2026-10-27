import { Link2, Upload, X } from "lucide-react";
import { useRef, useState } from "react";

import { Button } from "../../components/Button";
import { Field } from "../../components/Field";
import { IconButton } from "../../components/IconButton";
import { t } from "../../i18n/catalog";
import { currentPickerBackend } from "../pickers/pickerBackend";
import type { ArtworkChoice } from "./artworkApply";
import { choiceOfAddress, choiceOfImage } from "./artworkChoice";

interface ArtworkFooterProps {
  /** A picture of the user's: the picker shows it in the preview. */
  onChoice: (choice: ArtworkChoice) => void;
}

/**
 * The foot of the grid: "Use a file on this PC…" opens the
 * system dialog (a file field in a browser), "From a link…" opens a field for a pasted address.
 * Either one only shows the picture in the preview; "Use this" writes it.
 */
export function ArtworkFooter({ onChoice }: ArtworkFooterProps) {
  const input = useRef<HTMLInputElement>(null);
  const [link, setLink] = useState<string | null>(null);
  const [invalid, setInvalid] = useState(false);
  const [open, setOpen] = useState(false);

  const chooseFile = () => {
    const backend = currentPickerBackend();
    if (!backend.available) {
      input.current?.click();
      return;
    }
    if (open) return;
    setOpen(true);
    backend
      .pick("image", { title: t("artworkPicker.file.title") })
      .then((paths) => {
        const [path] = paths;
        if (path === undefined) return;
        const choice = choiceOfImage({ kind: "path", path });
        if (choice !== null) onChoice(choice);
      })
      .catch(() => undefined)
      .finally(() => setOpen(false));
  };

  const submitLink = () => {
    const choice = link === null ? null : choiceOfAddress(link);
    if (choice === null) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    setLink(null);
    onChoice(choice);
  };

  return (
    <div className="flex h-12 shrink-0 items-center gap-2 border-t border-solid border-line-section px-6">
      <Button icon={Upload} title={t("artworkPicker.file.tip")} onClick={chooseFile}>
        {t("artworkPicker.file")}
      </Button>
      {/* In a browser there is no system dialog: a file field stands in for it. */}
      <input
        ref={input}
        type="file"
        accept="image/*"
        tabIndex={-1}
        aria-label={t("artworkPicker.file.title")}
        className="sr-only"
        onChange={(event) => {
          const file = event.target.files?.[0];
          event.target.value = "";
          if (file === undefined) return;
          const choice = choiceOfImage({ kind: "file", file });
          if (choice !== null) onChoice(choice);
        }}
      />
      {link === null ? (
        <Button icon={Link2} onClick={() => setLink("")}>
          {t("artworkPicker.link")}
        </Button>
      ) : (
        <form
          className="flex min-w-0 flex-1 items-center gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            submitLink();
          }}
        >
          <Field
            type="text"
            autoComplete="off"
            spellCheck={false}
            mono
            aria-label={t("artworkPicker.link.label")}
            placeholder={t("artworkPicker.link.placeholder")}
            value={link}
            error={invalid ? t("artworkPicker.link.invalid") : undefined}
            onChange={(event) => {
              setLink(event.target.value);
              setInvalid(false);
            }}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.stopPropagation();
                setLink(null);
                setInvalid(false);
              }
            }}
            className="min-w-0 max-w-[460px] flex-1"
          />
          <Button type="submit">{t("artworkPicker.link.show")}</Button>
          <IconButton
            aria-label={t("artworkPicker.link.close")}
            icon={X}
            onClick={() => {
              setLink(null);
              setInvalid(false);
            }}
          />
        </form>
      )}
    </div>
  );
}
