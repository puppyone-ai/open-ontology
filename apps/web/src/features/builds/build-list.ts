import { callOperation, parseBuildList, type CallResult } from "@open-ontology/sdk-ts";
import type { ManagementSession } from "../../api/identity-session";

export const BUILD_ID = /^b_[0-9a-f]{32}$/;

export function adminProjectId(session: ManagementSession): string | null {
  const project = session.projects.find((item) => item.capabilities.includes("project.admin"));
  return project ? project.projectId : null;
}

export function loadBuildList(projectId: string): Promise<CallResult> {
  return callOperation({ operationId: "listBuilds", query: { project_id: projectId } });
}

export function loadBuildDetail(buildId: string): Promise<CallResult> {
  return callOperation({ operationId: "getBuild", params: { build_id: buildId } });
}

export type BuildRow = {
  buildId: string;
  projectId: string;
  status: string;
  stage: string;
};

export function readBuildRows(body: unknown): BuildRow[] {
  if (!parseBuildList(body).ok || !isRecord(body) || !Array.isArray(body.builds)) {
    return [];
  }
  const rows: BuildRow[] = [];
  for (const item of body.builds) {
    if (!isRecord(item)) {
      continue;
    }
    if (
      typeof item.build_id !== "string" ||
      typeof item.project_id !== "string" ||
      typeof item.status !== "string" ||
      typeof item.stage !== "string"
    ) {
      continue;
    }
    rows.push({
      buildId: item.build_id,
      projectId: item.project_id,
      status: item.status,
      stage: item.stage,
    });
  }
  return rows;
}

export function readNextCursor(body: unknown): string | null {
  if (!parseBuildList(body).ok || !isRecord(body) || !("next_cursor" in body) || typeof body.next_cursor !== "string") {
    return null;
  }
  return body.next_cursor;
}

export function readFlag(body: unknown, key: "published" | "active" | "compiled_pack"): string {
  if (!isRecord(body) || !(key in body)) {
    return "";
  }
  const value = body[key];
  if (typeof value === "boolean" || typeof value === "string") {
    return String(value);
  }
  return "";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
