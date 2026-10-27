import { createContext, useContext } from "react";

import type { EditSession } from "./useItemEdit";

/** The edit session the controls of the Edit details page read and write. */
export const SessionContext = createContext<EditSession | null>(null);

/** The page's edit session; a control outside the page is a programming error. */
export function useSession(): EditSession {
  const session = useContext(SessionContext);
  if (session === null) throw new Error("An edit control needs the page's edit session.");
  return session;
}
