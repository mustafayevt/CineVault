import { useEffect, useState } from "react";

import { currentUpdatesBackend } from "./updatesBackend";

/** The running CineVault version, or `""` until it is known (and in a plain browser). */
export function useAppVersion(): string {
  const [version, setVersion] = useState("");
  useEffect(() => {
    let live = true;
    currentUpdatesBackend()
      .status()
      .then((status) => {
        if (live) setVersion(status.current_version);
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);
  return version;
}
