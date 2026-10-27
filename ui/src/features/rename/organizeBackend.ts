import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  AppliedDto,
  JournalBatchDto,
  OrganizeCommandError,
  OrganizePreviewDto,
  OrganizeProfilesDto,
  OrganizeRequestDto,
  PatternFieldDto,
  PatternsDto,
  TemplateFindingDto,
  TemplateTokenDto,
  UndoOutcomeDto,
} from "../../bindings";
import { syntheticOrganizeBackend } from "./syntheticOrganize";

/** A refusal from the backend: `message` is ready to show, `code` is stable (see the bindings). */
class OrganizeRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "OrganizeRequestError";
    this.code = code;
  }
}

/** Where Organize gets its plans from. In the desktop app that is the Rust service. */
export interface OrganizeBackend {
  /** False in a plain browser, where the titles are samples and nothing is renamed on disk. */
  readonly available: boolean;
  profiles: () => Promise<OrganizeProfilesDto>;
  selectProfile: (id: string) => Promise<OrganizeProfilesDto>;
  saveProfile: (name: string, from: string, patterns: PatternsDto) => Promise<OrganizeProfilesDto>;
  /** Changes the templates of a custom profile; a preset cannot be changed. */
  updateProfile: (id: string, patterns: PatternsDto) => Promise<OrganizeProfilesDto>;
  deleteProfile: (id: string) => Promise<OrganizeProfilesDto>;
  /** The tokens of the movie templates, or the TV templates, for the `$` autocomplete. */
  tokens: (tv: boolean) => Promise<TemplateTokenDto[]>;
  checkTemplate: (field: PatternFieldDto, template: string) => Promise<TemplateFindingDto[]>;
  preview: (request: OrganizeRequestDto) => Promise<OrganizePreviewDto>;
  apply: (request: OrganizeRequestDto, include: readonly string[]) => Promise<AppliedDto>;
  journal: () => Promise<JournalBatchDto[]>;
  undo: (batchId: string) => Promise<UndoOutcomeDto>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: OrganizeCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new OrganizeRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriBackend: OrganizeBackend = {
  available: true,
  profiles: () => commands.organizeProfiles(),
  selectProfile: async (id) => unwrap(await commands.organizeSelectProfile(id)),
  saveProfile: async (name, from, patterns) =>
    unwrap(await commands.organizeSaveProfile(name, from, patterns)),
  updateProfile: async (id, patterns) => unwrap(await commands.organizeUpdateProfile(id, patterns)),
  deleteProfile: async (id) => unwrap(await commands.organizeDeleteProfile(id)),
  tokens: (tv) => commands.organizeTokens(tv),
  checkTemplate: (field, template) => commands.organizeCheckTemplate(field, template),
  preview: async (request) => unwrap(await commands.organizePreview(request)),
  apply: async (request, include) => unwrap(await commands.organizeApply(request, [...include])),
  journal: async () => unwrap(await commands.organizeJournal()),
  undo: async (batchId) => unwrap(await commands.organizeUndo(batchId)),
};

let override: OrganizeBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setOrganizeBackend(backend: OrganizeBackend | null): void {
  override = backend;
}

/** The backend in use: the override, the Rust service in the desktop app, else sample titles. */
export function currentOrganizeBackend(): OrganizeBackend {
  if (override !== null) return override;
  return isTauri() ? tauriBackend : syntheticOrganizeBackend;
}
