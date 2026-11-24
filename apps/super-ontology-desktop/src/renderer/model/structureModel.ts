import type { StructureClass, StructureProperty } from "../../shared/super-ontology/docs-api.generated";
import { invoke, type ApiResult } from "./bridgeInvoke";
import type { StructureSchema, StructureYaml } from "../../shared/super-ontology/docs-api.generated";

/** The structure layer of the ontology (D93): the classes of record files, like the table schemas of a database. */
export const structureApi = {
  read: (driveId: string): Promise<ApiResult<StructureSchema>> => invoke("structure-read", { driveId }),
  induce: (driveId: string): Promise<ApiResult<StructureSchema>> => invoke("structure-induce", { driveId }),
  decide: (input: { driveId: string; op: "accept" | "reject" | "rename" | "merge"; classKey: string; property?: string | null; name?: string; into?: string }): Promise<ApiResult<StructureSchema>> => invoke("structure-decide", input),
  yaml: (driveId: string, database?: boolean): Promise<ApiResult<StructureYaml>> => invoke("structure-yaml", database === undefined ? { driveId } : { driveId, database }),
};

export const KNOWN_KINDS = ["meeting_minutes", "decision_log", "weekly_report", "people_table", "project_table", "contract"] as const;

/** The class name the person sees: a known kind in their language, any other class by the label the engine found (its folder). */
export function isKnownKind(kind: string): kind is (typeof KNOWN_KINDS)[number] {
  return (KNOWN_KINDS as readonly string[]).includes(kind);
}

/** Properties that are not rejected, in the order the engine gives them (most common first). */
export function visibleProperties(c: StructureClass): StructureProperty[] {
  return c.properties.filter((p) => p.status !== "rejected");
}

/** Classes a person still has to look at come first; confirmed ones keep the engine's order. */
export function sortedClasses(classes: StructureClass[]): StructureClass[] {
  const rank = (c: StructureClass) => (c.status === "proposed" ? 0 : c.status === "confirmed" ? 1 : 2);
  return classes.filter((c) => c.status !== "rejected").map((c, i) => ({ c, i })).sort((a, b) => rank(a.c) - rank(b.c) || a.i - b.i).map((x) => x.c);
}

/** Up to three example values, cut to a readable length. */
export function exampleText(p: StructureProperty): string {
  return p.examples.slice(0, 3).map((e) => (e.length > 28 ? `${e.slice(0, 27)}…` : e)).join(" · ");
}

/** A relation names the class or entity type the values point at; `null` when the property is a plain value. */
export function relationTarget(p: StructureProperty, classes: StructureClass[]): { label: string; kind: "class" | "entity_type" } | null {
  const r = p.relation;
  if (!r) return null;
  const target = r.target_kind === "class" ? classes.find((c) => c.class_key === r.target) : undefined;
  return { label: target ? target.label : r.target, kind: r.target_kind === "class" ? "class" : "entity_type" };
}
