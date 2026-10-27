import { createContext, useContext } from "react";

/**
 * Which surface a control sits on. Inside a floating surface (menu, dialog, popover) fields and
 * lists use the one well colour; everywhere else they use the card colour. Menus, dialogs and
 * popovers wrap their content in `<SurfaceContext value="floating">`; a control can also be told
 * with its own `floating` prop.
 */
export type Surface = "page" | "floating";

export const SurfaceContext = createContext<Surface>("page");

/** The surface a control is on, optionally overridden by the control's own prop. */
export function useSurface(floating?: boolean): Surface {
  const inherited = useContext(SurfaceContext);
  if (floating === undefined) return inherited;
  return floating ? "floating" : "page";
}
