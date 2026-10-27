import type {
  AppliedDto,
  CompanionsDto,
  ConflictDto,
  JournalBatchDto,
  OrganizePreviewDto,
  OrganizeProfileDto,
  OrganizeProfilesDto,
  OrganizeRequestDto,
  PatternFieldDto,
  PreviewRowDto,
  TemplateFindingDto,
  TemplateTokenDto,
  UndoOutcomeDto,
} from "../../bindings";
import type { OrganizeBackend } from "./organizeBackend";
import { parseTemplate } from "./template";

/**
 * Sample titles for a plain browser (development and design review). The desktop app talks to the
 * Rust service instead. The rules here are a small stand-in for the template engine: simple
 * tokens, `${before,token,after}` and `${if token}`...`${end}`.
 */

interface SampleMovie {
  id: string;
  root: string;
  folder: string;
  file: string;
  title: string;
  year: number;
  edition: string;
  videoFormat: string;
  mediaSource: string;
  hdr: string;
  audioCodec: string;
  channels: string;
  tmdb: number;
  companions: CompanionsDto;
  locked?: boolean;
  /** The video details are not read yet: held back from the rename. */
  held?: boolean;
}

const NFO_ART = (artwork: number, subtitles = 0, trailers = 0): CompanionsDto => ({
  nfo: 1,
  artwork,
  subtitles,
  trailers,
  other: 0,
});

function movie(partial: Partial<SampleMovie> & Pick<SampleMovie, "id" | "folder" | "title">) {
  return {
    root: "F:\\Personal\\Movie\\",
    file: `${partial.folder}.mkv`,
    year: 2000,
    edition: "",
    videoFormat: "1080p",
    mediaSource: "BluRay",
    hdr: "",
    audioCodec: "DTS",
    channels: "5.1",
    tmdb: 0,
    companions: NFO_ART(3),
    ...partial,
  } satisfies SampleMovie;
}

function initialMovies(): SampleMovie[] {
  return [
    movie({
      id: "1",
      folder: "Dune.Part.Two.2024.2160p.WEB-DL.DV.HDR10.TrueHD.Atmos.7.1-GRP",
      title: "Dune: Part Two",
      year: 2024,
      videoFormat: "2160p",
      mediaSource: "WEB-DL",
      hdr: "DV HDR10",
      audioCodec: "TrueHD Atmos",
      channels: "7.1",
      tmdb: 693134,
      companions: NFO_ART(5, 1, 1),
    }),
    movie({
      id: "2",
      folder: "blade runner 2049 (2017) 4k",
      title: "Blade Runner 2049",
      year: 2017,
      videoFormat: "2160p",
      hdr: "HDR10",
      audioCodec: "DTS-HD MA",
      channels: "7.1",
      tmdb: 335984,
      companions: NFO_ART(4),
    }),
    movie({
      id: "3",
      root: "D:\\Media\\Movies\\",
      folder: "Heat.1995.Directors.Definitive.Edition.2160p",
      title: "Heat",
      year: 1995,
      edition: "Director's Definitive Edition",
      videoFormat: "2160p",
      hdr: "HDR10",
      audioCodec: "DTS-HD MA",
      channels: "5.1",
      tmdb: 949,
      companions: NFO_ART(4, 2),
    }),
    movie({
      id: "4",
      root: "D:\\Media\\Movies\\",
      folder: "Arrival (2016)",
      title: "Arrival",
      year: 2016,
      tmdb: 329865,
      companions: NFO_ART(3, 1),
    }),
    movie({
      id: "5",
      folder: "Spirited.Away.2001.JAPANESE.1080p",
      title: "Spirited Away",
      year: 2001,
      tmdb: 129,
      audioCodec: "DTS-HD MA",
      companions: NFO_ART(4, 3),
    }),
    movie({
      id: "6",
      folder: "Les.Miserables.2012.REPACK",
      title: "Les Misérables",
      year: 2012,
      tmdb: 82695,
    }),
    movie({
      id: "7",
      folder: "Les Miserables 2012 1080p",
      title: "Les Misérables",
      year: 2012,
      tmdb: 82695,
    }),
    movie({
      id: "9",
      root: "D:\\Media\\Movies\\",
      folder: "crash.2004.1080p.bluray",
      title: "Crash",
      year: 2004,
      tmdb: 1640,
      companions: NFO_ART(3),
    }),
    // The folder `Crash (2004)` exists already and belongs to another title: the rename above is blocked.
    movie({
      id: "10",
      root: "D:\\Media\\Movies\\",
      folder: "Crash (2004)",
      title: "Crash",
      year: 2004,
      tmdb: 1641,
      companions: NFO_ART(2),
    }),
    movie({
      id: "11",
      folder: "Mad.Max.Fury.Road.2015.2160p",
      title: "Mad Max: Fury Road",
      year: 2015,
      videoFormat: "2160p",
      tmdb: 76341,
      held: true,
    }),
    movie({
      id: "8",
      folder: "old cut",
      title: "Locked Title",
      year: 1999,
      locked: true,
    }),
  ];
}

const PRESETS: readonly OrganizeProfileDto[] = [
  {
    id: "clean",
    name: "Clean",
    builtin: true,
    patterns: {
      movie_folder: "${title} (${year})",
      movie_file: "${title}${ - ,edition,} (${year})",
      show_folder: "${showTitle} (${showYear})",
      season_folder: "Season ${seasonNr2}",
      episode_file: "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}",
    },
  },
  {
    id: "detailed",
    name: "Detailed",
    builtin: true,
    patterns: {
      movie_folder:
        "${title} (${year}) [${videoFormat}] [${mediaSource}] [${hdrformat}] [${audioCodec} ${audioChannelsDot}]",
      movie_file:
        "${title} (${year}) [${videoFormat}] [${mediaSource}] [${hdrformat}] [${audioCodec} ${audioChannelsDot}]",
      show_folder: "${showTitle} (${showYear})",
      season_folder: "Season ${seasonNr2}",
      episode_file:
        "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title} [${videoFormat}] [${mediaSource}] [${hdrformat}] [${audioCodec} ${audioChannelsDot}]",
    },
  },
  {
    id: "media-server-safe",
    name: "Media-server safe",
    builtin: true,
    patterns: {
      movie_folder: "${title} (${year})${if tmdb > 0} {tmdb-${tmdb}}${end}",
      movie_file: "${title} (${year})${if edition} {edition-${edition}}${end}",
      show_folder: "${showTitle} (${showYear})${if showTmdb > 0} {tmdb-${showTmdb}}${end}",
      season_folder: "Season ${seasonNr2}",
      episode_file: "${showTitle} - S${seasonNr2}E${episodeNr2}",
    },
  },
];

const MOVIE_TOKENS = [
  "title",
  "originalTitle",
  "year",
  "edition",
  "videoFormat",
  "videoCodec",
  "mediaSource",
  "hdrformat",
  "audioCodec",
  "audioChannelsDot",
  "tmdb",
  "imdb",
  "certification",
  "originalFilename",
];
const TV_TOKENS = [
  "showTitle",
  "showYear",
  "showTmdb",
  "seasonNr",
  "seasonNr2",
  "episodeNr",
  "episodeNr2",
  "title",
  "videoFormat",
  "mediaSource",
  "hdrformat",
  "audioCodec",
  "audioChannelsDot",
];

function valueOf(movieRow: SampleMovie, name: string): string {
  switch (name) {
    case "title":
      return movieRow.title;
    case "year":
      return String(movieRow.year);
    case "edition":
      return movieRow.edition;
    case "videoFormat":
      return movieRow.videoFormat;
    case "mediaSource":
      return movieRow.mediaSource;
    case "hdr":
    case "hdrformat":
    case "hdrFormat":
      return movieRow.hdr;
    case "audioCodec":
      return movieRow.audioCodec;
    case "audioChannelsDot":
      return movieRow.channels;
    case "tmdb":
      return movieRow.tmdb > 0 ? String(movieRow.tmdb) : "";
    default:
      return "";
  }
}

/** Renders a template for a sample movie, with the clean-up rules of the renamer. */
function render(template: string, movieRow: SampleMovie): string {
  return renderWith(template, (name) => valueOf(movieRow, name));
}

/** The sample episode the TV example shows: Severance, season 2 episode 1. */
function episodeValueOf(name: string): string {
  switch (name) {
    case "showTitle":
      return "Severance";
    case "showYear":
      return "2022";
    case "showTmdb":
      return "95396";
    case "seasonNr":
      return "2";
    case "seasonNr2":
      return "02";
    case "episodeNr":
      return "1";
    case "episodeNr2":
      return "01";
    case "title":
      return "Hello, Ms. Cobel";
    case "videoFormat":
      return "1080p";
    case "mediaSource":
      return "BluRay";
    case "hdr":
    case "hdrformat":
    case "hdrFormat":
      return "HDR10";
    case "audioCodec":
      return "DTS";
    case "audioChannelsDot":
      return "5.1";
    default:
      return "";
  }
}

function renderWith(template: string, lookup: (name: string) => string): string {
  const model = parseTemplate(template);
  let out = model.literals[0] ?? "";
  const skip: boolean[] = [];
  model.tokens.forEach((token, index) => {
    const literal = model.literals[index + 1] ?? "";
    let text = "";
    if (token.kind === "token") text = lookup(token.name);
    else if (token.kind === "optional") {
      const value = lookup(token.name);
      text = value === "" ? "" : token.prefix + value + token.suffix;
    } else {
      const inner = token.source.slice(2, -1).trim();
      if (inner.startsWith("if ")) {
        const name = inner.slice(3).split(/[\s>]/)[0] ?? "";
        skip.push(lookup(name) === "");
      } else if (inner === "end") {
        skip.pop();
      }
    }
    const hidden = skip.includes(true);
    out += (hidden ? "" : text) + (hidden ? "" : literal);
  });
  return out
    .replace(/: /g, " - ")
    .replace(/[<>"|?*]/g, "")
    .replace(/\s*(\[\]|\(\)|\{\})/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

interface SampleShow {
  id: string;
  title: string;
  year: number;
  before: string;
  after: string;
  episodes: number;
}

/** Two TV shows whose folders and episode files would be renamed, for the TV episodes tab. */
const SAMPLE_SHOWS: readonly SampleShow[] = [
  {
    id: "s1",
    title: "Severance",
    year: 2022,
    before: "Severance.S01.1080p.WEB-DL",
    after: "Severance (2022)",
    episodes: 9,
  },
  {
    id: "s2",
    title: "The Bear",
    year: 2022,
    before: "the.bear.s02.2160p",
    after: "The Bear (2022)",
    episodes: 10,
  },
];

function showRow(show: SampleShow): PreviewRowDto {
  const files = Array.from({ length: show.episodes }, (_, index) => {
    const number = String(index + 1).padStart(2, "0");
    return {
      kind: "episode",
      from: `${show.before}/${show.before}.E${number}.mkv`,
      to: [`${show.after}/Season 01/${show.title} - S01E${number}.mkv`],
    };
  });
  return {
    id: show.id,
    kind: "show",
    title: show.title,
    year: show.year,
    root: "E:\\Archive\\TV\\",
    before: show.before,
    after: show.after,
    before_file: "",
    after_file: "",
    status: "change",
    note: "",
    moves: show.episodes,
    trashed: 0,
    companions: { nfo: 0, artwork: 0, subtitles: 0, trailers: 0, other: 0 },
    files,
  };
}

interface Batch {
  id: string;
  createdMs: number;
  moves: { id: string; from: string; to: string }[];
  undone: boolean;
}

/** The in-memory sample backend. */
/**
 * The numbered name the backend offers for a conflict: `Crash (2004)` becomes `Crash (2004) (2)`,
 * `Crash (2004) (2)` becomes `Crash (2004) (3)`, and twins of one conflict get consecutive numbers.
 */
function numberedName(folder: string, twin: number): string {
  const found = /^(.*) \((\d{1,2})\)$/.exec(folder);
  const number = found === null ? 2 : Number(found[2]) + 1;
  return `${found?.[1] ?? folder} (${number + twin})`;
}

export function createSyntheticOrganizeBackend(): OrganizeBackend {
  const movies = initialMovies();
  let custom: OrganizeProfileDto[] = [];
  let active = "clean";
  const batches: Batch[] = [];
  let counter = 0;

  const profilesDto = (): OrganizeProfilesDto => ({ active, profiles: [...PRESETS, ...custom] });

  const preview = (request: OrganizeRequestDto): OrganizePreviewDto => {
    const selected =
      request.scope.kind === "selection"
        ? new Set(request.scope.ids)
        : new Set(movies.map((m) => m.id));
    const choices = new Map(request.choices.map((c) => [c.id, c]));
    const rows: PreviewRowDto[] = [];
    const counts = {
      change: 0,
      tidy: 0,
      conflicts: 0,
      locked: 0,
      skipped: 0,
      unplanned: 0,
      moves: 0,
    };
    const planned = movies
      .filter((m) => selected.has(m.id))
      .flatMap((m) => {
        if (m.locked === true) {
          counts.locked += 1;
          return [];
        }
        const choice = choices.get(m.id);
        let folder = render(request.patterns.movie_folder, m);
        const file = render(request.patterns.movie_file, m);
        if (choice?.kind === "edit" && choice.folder !== null) folder = choice.folder;
        if (folder === m.folder) {
          counts.tidy += 1;
          return [];
        }
        return [{ m, folder, file, choice }];
      });
    const byFolder = new Map<string, string[]>();
    for (const p of planned) {
      const key = p.folder.toLowerCase();
      byFolder.set(key, [...(byFolder.get(key) ?? []), p.m.id]);
    }
    const conflicts: ConflictDto[] = [];
    for (const [key, ids] of byFolder) {
      if (ids.length < 2) continue;
      const members = planned.filter((p) => ids.includes(p.m.id));
      const resolved = members.every(
        (p) => p.choice?.kind === "keep_as_versions" || p.choice?.kind === "skip",
      );
      if (resolved) continue;
      conflicts.push({
        code: "duplicate",
        owner: null,
        length: 0,
        limit: 0,
        items: ids,
        titles: members.map((p) => p.m.title),
        folders: members.map((p) => p.m.folder),
        path: members[0]?.folder ?? key,
        options: ["keep_as_versions", "skip", "edit"],
        suggestions: members.map((p, index) => numberedName(p.folder, index)),
      });
    }
    // A new name that another title's folder already has is blocked too.
    for (const p of planned) {
      if (p.choice?.kind === "skip") continue;
      const owner = movies.find(
        (m) =>
          m.id !== p.m.id &&
          m.root === p.m.root &&
          m.folder.toLowerCase() === p.folder.toLowerCase() &&
          !planned.some((q) => q.m.id === m.id),
      );
      if (owner === undefined) continue;
      conflicts.push({
        code: "foreign",
        owner: owner.title,
        length: 0,
        limit: 0,
        items: [p.m.id],
        titles: [p.m.title],
        folders: [p.m.folder],
        path: p.folder,
        options: ["skip", "edit"],
        suggestions: [numberedName(p.folder, 0)],
      });
    }
    const blocked = new Set(conflicts.flatMap((c) => c.items));
    for (const { m, folder, file, choice } of planned) {
      const skipped = choice?.kind === "skip";
      const status = skipped
        ? "skipped"
        : m.held === true
          ? "media_unread"
          : blocked.has(m.id)
            ? "conflict"
            : "change";
      const moves = Object.values(m.companions).reduce((sum, n) => sum + n, 0) + 1;
      rows.push({
        id: m.id,
        kind: "movie",
        title: m.title,
        year: m.year,
        root: m.root,
        before: m.folder,
        after: folder,
        before_file: m.file,
        after_file: `${file}.mkv`,
        status,
        note: status === "media_unread" ? "The names use video details that are not read yet." : "",
        moves,
        trashed: 0,
        companions: m.companions,
        files: [
          { kind: "video", from: `${m.folder}/${m.file}`, to: [`${folder}/${file}.mkv`] },
          ...(m.companions.nfo > 0
            ? [{ kind: "nfo", from: `${m.folder}/movie.nfo`, to: [`${folder}/${file}.nfo`] }]
            : []),
          ...(m.companions.artwork > 0
            ? [
                {
                  kind: "artwork",
                  from: `${m.folder}/poster.jpg`,
                  to: [`${folder}/${file}-poster.jpg`],
                },
              ]
            : []),
        ],
      });
      if (status === "change") {
        counts.change += 1;
        counts.moves += moves;
      } else if (status === "conflict") counts.conflicts += 1;
      else if (status === "skipped") counts.skipped += 1;
    }
    for (const show of SAMPLE_SHOWS) {
      if (request.scope.kind === "selection" && !request.scope.ids.includes(show.id)) continue;
      rows.push(showRow(show));
      counts.change += 1;
      counts.moves += show.episodes;
    }
    const example = (kind: string, source: SampleMovie) => ({
      kind,
      title: source.title,
      year: source.year,
      folder: render(request.patterns.movie_folder, source),
      file: render(request.patterns.movie_file, source),
      sample: false,
    });
    const render0 = (template: string) => renderWith(template, episodeValueOf);
    const first = movies[0];
    if (first === undefined) throw new Error("no sample titles");
    const find = (pick: (m: SampleMovie) => boolean) => movies.find(pick) ?? first;
    return {
      rows,
      conflicts,
      counts,
      examples: [
        example(
          "edition",
          find((m) => m.edition !== ""),
        ),
        example(
          "no_hdr",
          find((m) => m.hdr === ""),
        ),
        example(
          "colon",
          find((m) => m.title.includes(":")),
        ),
        {
          kind: "episode",
          title: "Severance",
          year: 2022,
          folder: `${render0(request.patterns.show_folder)}/${render0(request.patterns.season_folder)}`,
          file: render0(request.patterns.episode_file),
          sample: true,
        },
      ],
      problems: [],
    };
  };

  return {
    available: false,
    profiles: () => Promise.resolve(profilesDto()),
    selectProfile: (id) => {
      active = id;
      return Promise.resolve(profilesDto());
    },
    saveProfile: (name, from, patterns) => {
      const base = profilesDto().profiles.find((p) => p.id === from);
      const created: OrganizeProfileDto = {
        id: `custom-${name.toLowerCase().replace(/\W+/g, "-")}`,
        name,
        builtin: false,
        patterns: { ...(base?.patterns ?? PRESETS[0]?.patterns), ...patterns },
      };
      custom = [...custom, created];
      active = created.id;
      return Promise.resolve(profilesDto());
    },
    updateProfile: (id, patterns) => {
      custom = custom.map((p) => (p.id === id ? { ...p, patterns } : p));
      return Promise.resolve(profilesDto());
    },
    deleteProfile: (id) => {
      custom = custom.filter((p) => p.id !== id);
      if (active === id) active = "clean";
      return Promise.resolve(profilesDto());
    },
    tokens: (tv): Promise<TemplateTokenDto[]> =>
      Promise.resolve((tv ? TV_TOKENS : MOVIE_TOKENS).map((name) => ({ name, target: name }))),
    checkTemplate: (_field: PatternFieldDto, template: string): Promise<TemplateFindingDto[]> => {
      if (template.trim() === "") {
        return Promise.resolve([
          {
            severity: "error",
            message: "The template is empty, so every item would get the same name.",
            offset: 0,
            len: 0,
            suggestions: [],
          },
        ]);
      }
      return Promise.resolve([]);
    },
    preview: (request) => Promise.resolve(preview(request)),
    apply: (request, include) => {
      const plan = preview(request);
      const run = plan.rows.filter((r) => r.status === "change" && include.includes(r.id));
      if (run.length === 0) {
        return Promise.resolve<AppliedDto>({
          batch_id: null,
          done: 0,
          failed: 0,
          items: [],
          not_run: [...include],
        });
      }
      counter += 1;
      const batch: Batch = {
        id: `sample-${counter}`,
        createdMs: Date.now(),
        moves: [],
        undone: false,
      };
      for (const row of run) {
        const target = movies.find((m) => m.id === row.id);
        if (target === undefined) continue;
        batch.moves.push({ id: row.id, from: target.folder, to: row.after });
        target.folder = row.after;
      }
      batches.unshift(batch);
      return Promise.resolve<AppliedDto>({
        batch_id: batch.id,
        done: run.length,
        failed: 0,
        items: run.map((r) => ({
          id: r.id,
          title: r.title,
          status: "done",
          error: "",
          moved: r.moves,
          copied: 0,
          trashed: 0,
          notes: [],
        })),
        not_run: include.filter((id) => !run.some((r) => r.id === id)),
      });
    },
    journal: () =>
      Promise.resolve<JournalBatchDto[]>(
        batches.map((b) => ({
          batch_id: b.id,
          created_ms: b.createdMs,
          items: b.moves.length,
          operations: b.moves.length,
          done: b.undone ? 0 : b.moves.length,
          undone: b.undone ? b.moves.length : 0,
          failed: 0,
          undoable: !b.undone,
          undone_all: b.undone,
          files: b.moves.length,
          undone_ms: null,
          titles: b.moves.map((move) => movies.find((m) => m.id === move.id)?.title ?? ""),
        })),
      ),
    undo: (batchId) => {
      const batch = batches.find((b) => b.id === batchId);
      if (batch === undefined || batch.undone) {
        return Promise.reject(new Error("There is nothing left to undo in that batch."));
      }
      for (const move of batch.moves) {
        const target = movies.find((m) => m.id === move.id);
        if (target !== undefined) target.folder = move.from;
      }
      batch.undone = true;
      return Promise.resolve<UndoOutcomeDto>({
        batch_id: batchId,
        restored: batch.moves.length,
        not_restorable: [],
        failed: [],
      });
    },
  };
}

export const syntheticOrganizeBackend: OrganizeBackend = createSyntheticOrganizeBackend();
