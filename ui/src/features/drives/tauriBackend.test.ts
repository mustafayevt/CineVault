import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";

import type { DrivesOverviewDto } from "../../bindings";
import { tauriDrivesBackend } from "./drivesBackend";

afterEach(() => {
  clearMocks();
});

const wire: DrivesOverviewDto = {
  updatedMs: 1_700_000_000_000,
  drives: [
    {
      id: "E:",
      name: "E: Archive",
      kind: "ssd",
      // JSON cannot carry a number that is not finite: the generated types call it `null`.
      totalBytes: null,
      freeBytes: 400e9,
      folders: [
        {
          sourceId: 3,
          path: "E:/Archive/Movies",
          label: "Movies",
          type: "movie",
          titles: 39,
          bytes: 1.94e12,
        },
      ],
      notInLibraryBytes: null,
      quality: { p2160: 1, p1080: 2, p720: 3, sd: 4 },
      online: true,
      lastSeenMs: null,
      low: true,
      trashBytes: 2.4e9,
      binBytes: null,
      customTrash: null,
    },
  ],
};

describe("the desktop drives backend", () => {
  it("reads the overview from the drives_overview command", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return wire;
    });
    const overview = await tauriDrivesBackend.overview();
    expect(calls).toEqual(["drives_overview"]);
    expect(overview.updatedMs).toBe(1_700_000_000_000);
    const [drive] = overview.drives;
    expect(drive?.name).toBe("E: Archive");
    expect(drive?.low).toBe(true);
    expect(drive?.totalBytes).toBe(0);
    expect(drive?.folders[0]).toMatchObject({ sourceId: 3, type: "movie", titles: 39 });
  });

  it("says why when the library cannot be read", async () => {
    mockIPC(() => {
      throw { code: "storage", message: "The library could not be read." };
    });
    await expect(tauriDrivesBackend.overview()).rejects.toThrow("The library could not be read.");
  });
});
