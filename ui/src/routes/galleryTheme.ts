import { createContext, useContext } from "react";

/** The theme of the column being drawn, so a landmark can be named once per column. */
export const GalleryThemeContext = createContext<"dark" | "light">("dark");

/** A landmark name made unique across the two columns: "Toast demo (dark)". */
export function useColumnLabel(name: string): string {
  return `${name} (${useContext(GalleryThemeContext)})`;
}
