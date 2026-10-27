import { useMemo } from "react";

import { useDrivesOverview } from "../drives/drivesBackend";
import { shellDemo } from "./shellDemo";

/** One drive that holds a library folder, as the sidebar shows it. */
export interface SidebarDrive {
  id: string;
  /** "D: Media". */
  name: string;
  totalBytes: number;
  freeBytes: number;
  online: boolean;
  /** The low-space rule of spec `drives`, decided by the backend. */
  low: boolean;
}

const TB = 1e12;
const GB = 1e9;

/** The drives of the plain-browser demo (`?shell-demo`), the sample sidebar data. */
const DEMO_DRIVES: readonly SidebarDrive[] = [
  { id: "c", name: "C: System", totalBytes: 931 * GB, freeBytes: 41 * GB, online: true, low: true },
  {
    id: "d",
    name: "D: Media",
    totalBytes: 7.28 * TB,
    freeBytes: 1.21 * TB,
    online: true,
    low: false,
  },
  {
    id: "e",
    name: "E: Archive",
    totalBytes: 10.9 * TB,
    freeBytes: 5.81 * TB,
    online: true,
    low: false,
  },
  { id: "z", name: "Z: NAS", totalBytes: 16 * TB, freeBytes: 0, online: false, low: false },
];

const NONE: readonly SidebarDrive[] = [];

/**
 * The drives of the sidebar: the drives that hold a library folder, from the drives overview that
 * the Drives page and the Move to menu read too. Sample drives in the plain-browser
 * demo.
 */
export function useSidebarDrives(): readonly SidebarDrive[] {
  const overview = useDrivesOverview().data;
  const drives = useMemo(
    () =>
      overview === undefined
        ? NONE
        : overview.drives.map((drive) => ({
            id: drive.id,
            name: drive.name,
            totalBytes: drive.totalBytes,
            freeBytes: drive.freeBytes,
            online: drive.online,
            low: drive.low,
          })),
    [overview],
  );
  return shellDemo() ? DEMO_DRIVES : drives;
}
