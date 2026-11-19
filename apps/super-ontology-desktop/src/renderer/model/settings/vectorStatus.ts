import type { EngineSnapshot } from "../../../shared/super-ontology/bridge";

/** What the engine says about its local vector model (docs-api 1.5, F-S13-02). `unreported`: this engine does not say, which is not "off". */
export type VectorStatus =
  | { kind: "on"; model: string | null }
  | { kind: "off" }
  | { kind: "unavailable" }
  | { kind: "unreported" };

/**
 * The control API's health document has no vector field in docs-api 1.5; if a later engine exposes one, it is read from
 * `components.vector.status` (the component statuses of every other part) or from a top-level `vector` (the model version, or `null`
 * for off). Anything else is "unreported", and the page says so rather than guessing.
 */
export function readVectorStatus(health: EngineSnapshot["health"]): VectorStatus {
  const components = (health?.components ?? {}) as Record<string, { status?: string; version?: string } | undefined>;
  const component = components.vector;
  if (component && typeof component.status === "string") {
    if (component.status === "ok") return { kind: "on", model: typeof component.version === "string" ? component.version : null };
    if (component.status === "not_configured") return { kind: "off" };
    return { kind: "unavailable" };
  }
  const top = (health as { vector?: unknown } | null)?.vector;
  if (typeof top === "string" && top !== "") return { kind: "on", model: top };
  if (top === null) return { kind: "off" };
  return { kind: "unreported" };
}
