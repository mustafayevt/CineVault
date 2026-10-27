import type { HistoryEntryDto, HistoryTitleDto } from "../../bindings";
import { HistoryError } from "./historyBackend";
import type { HistoryBackend, HistoryFilter } from "./historyBackend";

const GB = 1_000_000_000;
const HOUR = 3_600_000;

const KINDS: Record<HistoryFilter, readonly string[]> = {
  all: [],
  renames: ["rename"],
  moves: ["move"],
  deletes: ["delete", "delete_permanent"],
  artwork_and_matches: ["artwork", "match"],
};

const RENAMES: readonly [string, string][] = [
  [
    String.raw`D:\Movies\Dune.Part.Two.2024.2160p.UHD.BluRay.x265-HDR`,
    String.raw`D:\Movies\Dune Part Two (2024)`,
  ],
  [
    String.raw`D:\Movies\Blade.Runner.2049.2017.UHD.BluRay.2160p`,
    String.raw`D:\Movies\Blade Runner 2049 (2017)`,
  ],
  [
    String.raw`D:\Movies\Mad Max Fury Road 2015 2160p`,
    String.raw`D:\Movies\Mad Max - Fury Road (2015)`,
  ],
  [
    String.raw`E:\Archive\Movies\the.batman.2022.2160p.uhd.bluray.x265-HDR`,
    String.raw`E:\Archive\Movies\The Batman (2022)`,
  ],
  [String.raw`D:\Movies\Sicario (2015) 1080p`, String.raw`D:\Movies\Sicario (2015)`],
  [String.raw`D:\Movies\Heat.1995.1080p`, String.raw`D:\Movies\Heat (1995)`],
  [String.raw`D:\Movies\Arrival.2016.2160p`, String.raw`D:\Movies\Arrival (2016)`],
];

function title(id: string, before: string, after: string, name: string): HistoryTitleDto {
  return { id, title: name, before, after, state: "done" };
}

/** The sample titles of each sample entry, for "Show N more". */
const TITLES: Record<string, HistoryTitleDto[]> = {
  "1": RENAMES.map(([before, after], index) =>
    title(String(10 + index), before, after, after.split("\\").pop() ?? after),
  ),
};

function entry(
  id: string,
  kind: string,
  agoMs: number,
  summary: string,
  detail: string,
  rest: Partial<HistoryEntryDto> = {},
): HistoryEntryDto {
  const preview = TITLES[id] ?? [];
  return {
    id,
    kind,
    at: Date.now() - agoMs,
    summary,
    detail,
    bytes: null,
    batch_id: null,
    undone_at: null,
    undo_state: kind === "delete_permanent" ? "none" : "available",
    undo_note: null,
    titles_total: Math.max(preview.length, 1),
    titles_undone: 0,
    preview: preview.slice(0, 5),
    ...rest,
  };
}

/** The sample History: newest first. */
function sampleEntries(): HistoryEntryDto[] {
  return [
    entry(
      "1",
      "rename",
      20 * 60_000,
      "Renamed 7 titles",
      String.raw`7 folders, 41 files · D:\Movies, E:\Archive\Movies`,
    ),
    entry(
      "2",
      "move",
      HOUR,
      "Moved 3 titles from D: to E:",
      "Oppenheimer, Blade Runner 2049, Arrival · 198.7 GB · copied and checked",
      {
        bytes: 198.7 * GB,
        titles_total: 3,
        preview: [
          title("20", "D:", "E:", "Oppenheimer"),
          title("21", "D:", "E:", "Blade Runner 2049"),
          title("22", "D:", "E:", "Arrival"),
        ],
      },
    ),
    entry(
      "3",
      "artwork",
      2 * HOUR,
      "Changed poster · Dune: Part Two",
      "poster.jpg kept in the library folder's trash folder",
    ),
    entry(
      "4",
      "delete",
      3 * HOUR,
      "Deleted Godzilla (1998) to the Recycle Bin",
      String.raw`E:\Archive\Movies\GDZ_final_v2 · 12.1 GB`,
      { bytes: 12.1 * GB },
    ),
    entry("5", "rename", 3.5 * HOUR, "Renamed Solaris", "Undone at 11:30", {
      undo_state: "undone",
      undone_at: Date.now() - 3 * HOUR,
      titles_undone: 1,
    }),
    entry(
      "6",
      "match",
      30 * HOUR,
      "Changed match · Heat (1986)",
      "Heat (1986) → Heat (1995) · NFO and 8 artwork files replaced",
    ),
    entry(
      "7",
      "delete_permanent",
      33 * HOUR,
      "Permanently deleted 2 titles",
      "Cats (2019), The Room (2003) · 18.4 GB · cannot be undone",
      {
        bytes: 18.4 * GB,
        titles_total: 2,
        preview: [
          title("30", String.raw`E:\Archive\Movies\Cats (2019)`, "", "Cats (2019)"),
          title("31", String.raw`E:\Archive\Movies\The Room (2003)`, "", "The Room (2003)"),
        ],
      },
    ),
    entry(
      "8",
      "rename",
      36 * HOUR,
      "Renamed 38 episodes · Severance",
      String.raw`E:\TV\Severance (2022)`,
    ),
  ];
}

/** A History of sample entries for a browser with no library; Undo and Delete change it. */
export function createSyntheticHistoryBackend(): HistoryBackend {
  let entries = sampleEntries();
  let next = 100;
  const handlers = new Set<() => void>();
  const changed = () => handlers.forEach((handler) => handler());

  const undo = (entryId: string, only?: string) => {
    const found = entries.find((item) => item.id === entryId);
    if (found === undefined)
      throw new HistoryError("unknown_entry", "This entry no longer exists.");
    if (found.undo_state === "none")
      throw new HistoryError("refused", "A permanent delete cannot be undone.");
    if (found.undo_state === "undone")
      throw new HistoryError("refused", "This change is undone already.");
    const wholly = only === undefined;
    const undone = wholly ? found.preview.map((item) => item.id) : [only];
    entries = entries.map((item) =>
      item.id === entryId
        ? {
            ...item,
            undo_state: wholly ? "undone" : "partial",
            undone_at: wholly ? Date.now() : null,
            titles_undone: wholly ? item.titles_total : item.titles_undone + 1,
          }
        : item,
    );
    changed();
    return {
      entry_id: entryId,
      undone,
      left: [],
      undo_state: wholly ? "undone" : "partial",
      undone_at: wholly ? Date.now() : null,
    };
  };

  const record = (kind: string, summary: string, detail: string, bytes: number | null) => {
    next += 1;
    const id = String(next);
    entries = [
      entry(id, kind, 0, summary, detail, {
        bytes,
        ...(kind === "delete_permanent" ? { undo_state: "none" } : {}),
      }),
      ...entries,
    ];
    changed();
    return id;
  };

  return {
    list: (filter, offset, limit) => {
      const kinds = KINDS[filter];
      const all = entries.filter((item) => kinds.length === 0 || kinds.includes(item.kind));
      return Promise.resolve({ entries: all.slice(offset, offset + limit), total: all.length });
    },
    titles: (entryId) =>
      Promise.resolve(
        TITLES[entryId] ?? entries.find((item) => item.id === entryId)?.preview ?? [],
      ),
    forTitle: (id) =>
      Promise.resolve(
        id === "1"
          ? [
              entry("50", "rename", 2 * HOUR, "Renamed 7 titles", "7 folders", {
                titles_total: 7,
                preview: [
                  title(
                    "1",
                    "Dune.Part.Two.2024.2160p.UHD.BluRay.x265-HDR",
                    "Dune Part Two (2024)",
                    "Dune: Part Two",
                  ),
                ],
              }),
            ]
          : [],
      ),
    entryForBatch: () => Promise.resolve(null),
    binTotal: () => Promise.resolve(214.6 * GB),
    undo: (entryId) => Promise.resolve().then(() => undo(entryId)),
    undoTitle: (entryId, id) => Promise.resolve().then(() => undo(entryId, id)),
    deleteTitles: (ids) => {
      const entryId = record(
        "delete",
        ids.length === 1
          ? "Deleted 1 title to the Recycle Bin"
          : `Deleted ${ids.length} titles to the Recycle Bin`,
        "",
        null,
      );
      return Promise.resolve({
        deleted: ids.map((id) => ({
          id,
          title: id,
          went_to: "recycle_bin",
          notes: [],
        })),
        refused: [],
        entry_id: entryId,
      });
    },
    eraseTitles: (ids, confirm) => {
      if (confirm !== "DELETE")
        return Promise.reject(
          new HistoryError("confirmation", "Type DELETE to erase these titles for good."),
        );
      const entryId = record(
        "delete_permanent",
        `Permanently deleted ${ids.length} titles`,
        "cannot be undone",
        null,
      );
      return Promise.resolve({
        erased: ids.map((id) => ({ id, title: id, erased: [], left: [], error: null })),
        stopped: [],
        refused: [],
        entry_id: entryId,
      });
    },
    onChanged: (handler) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
  };
}
