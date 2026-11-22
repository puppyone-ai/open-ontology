import type { EngineFileProfile, EngineProfileOverride } from "../../shared/super-ontology/bridge";
import { invoke, type ApiResult } from "./bridgeInvoke";

/** The person's choice of class for one file or a folder (D94). `profile: null` hands the decision back to the engine. */
export function setProfileOverride(input: { driveId: string; scope: "file" | "dir"; path: string; profile: EngineFileProfile | null }): Promise<ApiResult<{ affected: number }>> {
  return invoke("profile-override-set", input);
}

export async function listProfileOverrides(driveId: string): Promise<ApiResult<EngineProfileOverride[]>> {
  const result = await invoke("profile-overrides-list", { driveId });
  return result.ok ? { ok: true, value: result.value.items } : result;
}
