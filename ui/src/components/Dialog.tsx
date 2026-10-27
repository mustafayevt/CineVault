import { X } from "lucide-react";
import { Dialog as RadixDialog } from "radix-ui";
import type { ReactElement, ReactNode, RefObject } from "react";

import { t } from "../i18n/catalog";
import { cx } from "../lib/cx";
import { floatingSurface, useFloatingContainer } from "./floating";
import { IconButton } from "./IconButton";
import { SurfaceContext } from "./surface";

interface DialogProps {
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  /** An optional control that opens the dialog; focus returns to it on close. */
  trigger?: ReactElement;
  /** The heading, 16/600; it also names the dialog. */
  title: ReactNode;
  /** One line under the title; it also describes the dialog. */
  description?: ReactNode;
  children?: ReactNode;
  /** The commit row at the bottom right: large (32px) buttons, the primary one last. */
  footer?: ReactNode;
  /** Width classes; 440px by default. */
  className?: string;
  /** Hides the close button at the top right (Esc still closes). */
  hideClose?: boolean;
  /** The control that takes focus on open; the first focusable one when not given. */
  initialFocus?: RefObject<HTMLElement | null>;
}

/**
 * The one dialog (the keyboard-shortcut sheet, delete confirmations): the floating recipe with
 * radius 14, centred over a scrim, focus kept inside while open and handed back on close, Esc and
 * the close button dismiss it. It fades in and grows from 0.98 (`.cv-pop`); the scrim fades.
 */
export function Dialog({
  open,
  defaultOpen,
  onOpenChange,
  trigger,
  title,
  description,
  children,
  footer,
  className,
  hideClose,
  initialFocus,
}: DialogProps) {
  const container = useFloatingContainer();
  return (
    <RadixDialog.Root open={open} defaultOpen={defaultOpen} onOpenChange={onOpenChange}>
      {trigger !== undefined && <RadixDialog.Trigger asChild>{trigger}</RadixDialog.Trigger>}
      <RadixDialog.Portal container={container}>
        <RadixDialog.Overlay className="cv-xfade fixed inset-0 z-50 grid place-items-center overflow-y-auto bg-scrim p-6">
          <RadixDialog.Content
            {...(description === undefined ? { "aria-describedby": undefined } : {})}
            {...(initialFocus === undefined
              ? {}
              : {
                  onOpenAutoFocus: (event: Event) => {
                    if (initialFocus.current === null) return;
                    event.preventDefault();
                    initialFocus.current.focus();
                  },
                })}
            className={cx(
              floatingSurface,
              "relative flex w-[440px] max-w-full flex-col gap-4 rounded-(--r-14) p-5",
              className,
            )}
          >
            <SurfaceContext value="floating">
              <div className="flex items-start gap-3 pr-8">
                <div className="flex min-w-0 flex-col gap-1">
                  <RadixDialog.Title className="text-(length:--fs-16) font-(weight:--fw-600) text-text">
                    {title}
                  </RadixDialog.Title>
                  {description !== undefined && (
                    <RadixDialog.Description className="text-(length:--fs-13) text-text-2">
                      {description}
                    </RadixDialog.Description>
                  )}
                </div>
              </div>
              {hideClose !== true && (
                <RadixDialog.Close asChild>
                  <IconButton
                    aria-label={t("common.close")}
                    icon={X}
                    className="absolute top-3 right-3"
                  />
                </RadixDialog.Close>
              )}
              {children}
              {footer !== undefined && (
                <div className="flex items-center justify-end gap-2 pt-1">{footer}</div>
              )}
            </SurfaceContext>
          </RadixDialog.Content>
        </RadixDialog.Overlay>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}
