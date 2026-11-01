import { Trash2 } from "lucide-react";
import type { ReactNode } from "react";

import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import { t } from "../../i18n/catalog";

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
  onConfirm: () => void;
  onClose: () => void;
}

/**
 * The Yes or No before files are erased for good (spec `title-deletion` "A permanent delete asks
 * Yes or No", "Delete permanently as the choice", `drives` "Empty trash folders with a Yes or No"):
 * the title with a red bin, a line that says it cannot be undone, the list, and Cancel and the red
 * button.
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
            disabled={!canConfirm}
            onClick={onConfirm}
            data-testid="confirm-erase"
          >
            {confirmLabel}
          </Button>
        </div>
      }
    >
      {children}
    </Dialog>
  );
}
