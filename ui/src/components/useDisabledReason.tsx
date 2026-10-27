import { useCallback, useId, useState } from "react";
import type { ReactNode } from "react";

import { ReasonTip } from "./DisabledReason";

/**
 * A disabled control explains itself. It stays focusable (`aria-disabled`, never the `disabled`
 * attribute), and on hover and keyboard focus shows one line: the reason, then the action that
 * enables it, for example "Needs a match first · Scrape…". Escape hides the line again.
 */
export interface DisabledReasonProps {
  /** Why the control cannot act now. Setting it makes the control disabled. */
  disabledReason?: string;
  /** The action that enables it, shown after the reason. */
  enabledBy?: string;
}

/** The one line shown for a reason: "Needs a match first · Scrape…". */
export function reasonLine(reason: string, enabledBy?: string): string {
  return enabledBy === undefined || enabledBy === "" ? reason : `${reason} · ${enabledBy}`;
}

/** Handlers that take no event data, so any element's events can call them. */
interface Handlers {
  onMouseEnter: () => void;
  onMouseLeave: () => void;
  onFocus: () => void;
  onBlur: () => void;
  onKeyDown: (event: { key: string }) => void;
}

interface DisabledReasonState {
  /** True when a reason is set: the control must ignore activation. */
  disabled: boolean;
  /** Props for the control itself; empty when it is enabled. */
  controlProps: Partial<Handlers> & { "aria-disabled"?: true; "aria-describedby"?: string };
  /** The tooltip element to render beside or inside the control, or null while hidden. */
  tip: ReactNode;
}

/** State and props for a control that can be disabled with a reason. */
export function useDisabledReason({
  disabledReason,
  enabledBy,
}: DisabledReasonProps): DisabledReasonState {
  const [open, setOpen] = useState(false);
  const id = useId();
  const show = useCallback(() => setOpen(true), []);
  const hide = useCallback(() => setOpen(false), []);
  const onKeyDown = useCallback((event: { key: string }) => {
    if (event.key === "Escape") setOpen(false);
  }, []);
  if (disabledReason === undefined) return { disabled: false, controlProps: {}, tip: null };
  return {
    disabled: true,
    controlProps: {
      "aria-disabled": true,
      "aria-describedby": id,
      onMouseEnter: show,
      onMouseLeave: hide,
      onFocus: show,
      onBlur: hide,
      onKeyDown,
    },
    tip: open ? <ReasonTip id={id} line={reasonLine(disabledReason, enabledBy)} /> : null,
  };
}
