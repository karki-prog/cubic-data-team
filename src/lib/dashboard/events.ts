import type { TeamTool } from "@/lib/content/tools";

export const CUBIC_TOOL_EVENT = "cubic-tool";

export function requestOpenTool(tool: TeamTool) {
  window.dispatchEvent(new CustomEvent(CUBIC_TOOL_EVENT, { detail: tool }));
}
