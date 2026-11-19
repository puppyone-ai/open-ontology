import type { EntityRef, Fact } from "../../../shared/super-ontology/docs-api.generated";

/** Entity types in the order the entities tab lists them (people first); anything else goes to "other". */
export const ENTITY_TYPE_ORDER = ["Person", "Organization", "Project", "Decision", "Document", "Topic", "Event", "Term"] as const;
export type EntityGroupId = (typeof ENTITY_TYPE_ORDER)[number] | "other";

export function groupEntities<T extends Pick<EntityRef, "type">>(entities: readonly T[]): { group: EntityGroupId; items: T[] }[] {
  const groups = new Map<EntityGroupId, T[]>();
  for (const entity of entities) {
    const group: EntityGroupId = (ENTITY_TYPE_ORDER as readonly string[]).includes(entity.type) ? (entity.type as EntityGroupId) : "other";
    groups.set(group, [...(groups.get(group) ?? []), entity]);
  }
  return [...ENTITY_TYPE_ORDER, "other" as const].filter((group) => groups.has(group)).map((group) => ({ group, items: groups.get(group) as T[] }));
}

/** `valid_precision` decides how much of an ISO date is shown: a month fact says "2026-09", not "2026-09-01". */
export function formatValidity(value: string | null | undefined, precision: string | undefined): string {
  if (!value) return "";
  if (precision === "year") return value.slice(0, 4);
  if (precision === "month") return value.slice(0, 7);
  return value.slice(0, 10);
}

export type FactBucket = "current" | "superseded" | "retracted";

/** Current facts are published or still waiting; a fact that was replaced is "superseded"; a withdrawn or rejected one is "retracted" (hidden unless asked for, D17). */
export function bucketOf(fact: Pick<Fact, "status" | "retracted_at" | "superseded_by">): FactBucket {
  if (fact.status === "retracted" || fact.status === "rejected" || fact.retracted_at) return "retracted";
  if (fact.superseded_by) return "superseded";
  return "current";
}

export const confidenceOf = (fact: Pick<Fact, "confidence_bp">): number => fact.confidence_bp / 10_000;

/** The object of a fact in words: another entity's name, or the first text of the literal value. */
export function factObjectText(fact: Pick<Fact, "object" | "value">): string {
  if (fact.object?.name) return fact.object.name;
  const value = fact.value;
  if (!value) return "";
  const text = Object.values(value).find((part) => typeof part === "string");
  return typeof text === "string" ? text : JSON.stringify(value);
}
