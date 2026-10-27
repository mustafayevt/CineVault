import { Trash2 } from "lucide-react";
import { useId, useState } from "react";
import type { ReactNode } from "react";

import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import { Field } from "../../components/Field";
import { t } from "../../i18n/catalog";

/** The word that has to be typed. The backend checks it again. */
export const ERASE_WORD = "DELETE";

interface EraseConfirmDialogProps {
  title: string;
  /** One line under the title: what is erased and that it cannot be undone. */
  description: ReactNode;
  /** The list of what is erased (a well), or nothing. */
  children?: ReactNode;
  /** The red button's label. */
  confirmLabel: string;
  /** Off while there is nothing to erase or the list is still read. */
  canConfirm?: boolean;
  onConfirm: (typed: string) => void;
  onClose: () => void;
}

/**
 * The typed confirmation of the permanent delete, for the other places that erase files for good
 * (spec `drives` "Empty a drive's trash", `title-deletion` "Delete permanently as the choice"):
 * the title with a red bin, a line that says it cannot be undone, the list, a field for DELETE,
 * and Cancel and the red button, which stays off until the field holds exactly DELETE.
 */
export function EraseConfirmDialog({
  title,
  description,
  children,
  confirmLabel,
  canConfirm = true,
  onConfirm,
  onClose,
}: EraseConfirmDialogProps) {
  const [typed, setTyped] = useState("");
  const field = useId();
  const ready = typed === ERASE_WORD && canConfirm;
  const confirm = () => {
    if (ready) onConfirm(typed);
  };
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      hideClose
      className="w-[520px]!"
      title={
        <span className="flex items-center gap-2">
          <Trash2
            aria-hidden="true"
            size={18}
            strokeWidth={1.75}
            className="shrink-0 text-danger"
          />
          {title}
        </span>
      }
      description={description}
      footer={
        <div className="flex w-full items-center justify-end gap-2">
          <Button size="lg" onClick={onClose}>
            {t("delete.dialog.cancel")}
          </Button>
          <Button
            size="lg"
            variant="danger"
            disabled={!ready}
            onClick={confirm}
            data-testid="confirm-erase"
          >
            {confirmLabel}
          </Button>
        </div>
      }
    >
      {children}
      <div className="flex flex-col gap-1.5">
        <label htmlFor={field} className="text-text-2">
          {t("delete.dialog.typeBefore")}{" "}
          <b className="font-mono font-(weight:--fw-700) text-text">{ERASE_WORD}</b>{" "}
          {t("delete.dialog.typeAfter")}
        </label>
        <Field
          id={field}
          mono
          value={typed}
          onChange={(event) => setTyped(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") confirm();
          }}
          autoComplete="off"
          spellCheck={false}
          aria-label={t("delete.dialog.field")}
        />
      </div>
    </Dialog>
  );
}
