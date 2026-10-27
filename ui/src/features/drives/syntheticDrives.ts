import type { Drive, DrivesOverview, TrashEntry } from "./drivesModel";

const TB = 1e12;
const GB = 1e9;
const DAY_MS = 86_400_000;

/** What Empty… lists for a drive of the sample: its trash folder's batches and its bin items. */
export function syntheticTrash(drive: Drive): TrashEntry[] {
  const root = drive.folders[0]?.path ?? drive.id;
  const entries: TrashEntry[] = [];
  if (drive.trashBytes > 0) {
    entries.push(
      {
        kind: "folder",
        path: `${root}\\.deletedByCineVault\\1761400000000-0`,
        bytes: drive.trashBytes * 0.75,
      },
      {
        kind: "folder",
        path: `${root}\\.deletedByCineVault\\artwork-1761500000000-3`,
        bytes: drive.trashBytes * 0.25,
      },
    );
  }
  if (drive.binBytes > 0) {
    entries.push({ kind: "bin", path: `${root}\\Heat (1995)`, bytes: drive.binBytes });
  }
  return entries;
}

/**
 * The drives of the plain-browser demo, `bun run shots` and the tests: four sample
 * drives (one low, one offline). The library folder labels match the synthetic
 * library's, so choosing a drive lists titles.
 */
export function syntheticDrives(now: number = Date.now()): DrivesOverview {
  const drives: Drive[] = [
    {
      id: "D:",
      name: "D: Media",
      kind: "hdd",
      totalBytes: 7.28 * TB,
      freeBytes: 1.21 * TB,
      folders: [
        {
          sourceId: 1,
          path: String.raw`D:\Movies`,
          label: "Movies",
          type: "movie",
          titles: 142,
          bytes: 5.12 * TB,
        },
      ],
      notInLibraryBytes: 0.95 * TB,
      quality: { p2160: 38, p1080: 89, p720: 12, sd: 3 },
      online: true,
      lastSeenMs: now,
      low: false,
      trashBytes: 2.4 * GB,
      binBytes: 3.6 * GB,
      customTrash: null,
    },
    {
      id: "E:",
      name: "E: Archive",
      kind: "hdd",
      totalBytes: 10.9 * TB,
      freeBytes: 5.81 * TB,
      folders: [
        {
          sourceId: 2,
          path: String.raw`E:\Archive\Movies`,
          label: "Archive",
          type: "movie",
          titles: 39,
          bytes: 1.94 * TB,
        },
        {
          sourceId: 3,
          path: String.raw`E:\TV`,
          label: "TV",
          type: "tv",
          titles: 18,
          bytes: 3.15 * TB,
        },
      ],
      notInLibraryBytes: null,
      quality: { p2160: 23, p1080: 29, p720: 5, sd: 0 },
      online: true,
      lastSeenMs: now,
      low: false,
      trashBytes: 0,
      binBytes: 0,
      customTrash: null,
    },
    {
      id: "C:",
      name: "C: System",
      kind: "ssd",
      totalBytes: 931 * GB,
      freeBytes: 41 * GB,
      folders: [],
      notInLibraryBytes: 212 * GB,
      quality: { p2160: 0, p1080: 0, p720: 0, sd: 0 },
      online: true,
      lastSeenMs: now,
      low: true,
      trashBytes: 0,
      binBytes: 0,
      customTrash: null,
    },
    {
      id: "Z:",
      name: "Z: NAS",
      kind: "network",
      totalBytes: 16 * TB,
      freeBytes: 0,
      folders: [
        {
          sourceId: 4,
          path: String.raw`Z:\Shows`,
          label: "Shows",
          type: "tv",
          titles: 5,
          bytes: 1.06 * TB,
        },
      ],
      notInLibraryBytes: null,
      quality: { p2160: 0, p1080: 5, p720: 0, sd: 0 },
      online: false,
      lastSeenMs: now - 2 * DAY_MS,
      low: false,
      trashBytes: 0,
      binBytes: 0,
      customTrash: null,
    },
  ];
  return { drives, updatedMs: now };
}
