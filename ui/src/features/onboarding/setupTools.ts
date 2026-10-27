import { t } from "../../i18n/catalog";
import { showToast } from "../toasts/toastStore";
import { currentToolsBackend } from "../tools/toolsBackend";

/** The tools the summary offers to download: they read the video details and make thumbnails. */
const RECOMMENDED_TOOLS = ["mediainfo", "ffmpeg"] as const;

/**
 * Works out which of the recommended tools (and `extraTools`, the tools an answer of the setup
 * asks for) are not installed yet, and tells the backend they are on their way, so the first scan
 * waits for them instead of reporting them missing. A tool that is in both lists is counted once.
 * `includeRecommended` is false when the person unticked the recommended tools but still needs
 * the extra ones. Returns the ids to download; call {@link downloadTools} with them.
 */
export async function queueRecommendedTools(
  extraTools: readonly string[] = [],
  includeRecommended = true,
): Promise<QueuedTools> {
  const backend = currentToolsBackend();
  if (!backend.available) return [];
  let list;
  try {
    list = await backend.list();
  } catch {
    return [];
  }
  const wanted = [...new Set([...(includeRecommended ? RECOMMENDED_TOOLS : []), ...extraTools])];
  const queued = wanted.flatMap((id) => {
    const tool = list.find((entry) => entry.id === id);
    if (tool === undefined || tool.state === "ready" || tool.state === "unsupported") return [];
    return [{ id, name: tool.name }];
  });
  if (queued.length > 0) {
    try {
      await backend.expect?.(queued.map((tool) => tool.id));
    } catch {
      // Only a hint for the backend: the downloads run anyway.
    }
  }
  return queued;
}

/** The tools {@link queueRecommendedTools} found missing, in download order. */
export type QueuedTools = readonly { id: string; name: string }[];

/**
 * Downloads `tools` one after the other, in the background. A failure is a toast and never stops
 * the setup (spec `onboarding` "Tools download after setup").
 */
export async function downloadTools(tools: QueuedTools): Promise<void> {
  const backend = currentToolsBackend();
  for (const tool of tools) {
    try {
      await backend.install(tool.id, () => undefined);
    } catch (error) {
      showToast({
        tone: "error",
        message: `${t("setup.tools.failed", { name: tool.name })} ${
          error instanceof Error ? error.message : String(error)
        }`,
      });
    }
  }
}

/** {@link queueRecommendedTools} and then {@link downloadTools}. */
export async function downloadRecommendedTools(
  extraTools: readonly string[] = [],
  includeRecommended = true,
): Promise<void> {
  await downloadTools(await queueRecommendedTools(extraTools, includeRecommended));
}
