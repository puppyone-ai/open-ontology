import type { ReviewGroup, ReviewItem } from "../../../shared/super-ontology/docs-api.generated";

/** The review kinds with their glyph, tone and the decisions each one offers (20-frontend section 4.3). */
export type ReviewKindId = ReviewItem["kind"];
export type ReviewIconName = "CircleHelp" | "Link2" | "ArrowLeftRight" | "PencilLine" | "PackagePlus" | "MessageSquare" | "RefreshCw" | "TriangleAlert" | "Mic";
export type ReviewTone = "info" | "warning" | "neutral";

export type ReviewKindView = {
  kind: ReviewKindId | "unknown";
  icon: ReviewIconName;
  tone: ReviewTone;
  messageId: string;
  /** A whole-source accept may include this kind. Kinds that need a person's choice (a name, a winner, a type) never do. */
  bulkAccept: boolean;
  bulkReject: boolean;
  /** The row can be decided with the plain accept and reject keys. */
  simpleDecisions: boolean;
};

const KINDS: Record<ReviewKindId, Omit<ReviewKindView, "kind" | "messageId">> = {
  low_confidence_fact: { icon: "CircleHelp", tone: "info", bulkAccept: true, bulkReject: true, simpleDecisions: true },
  entity_merge: { icon: "Link2", tone: "info", bulkAccept: true, bulkReject: true, simpleDecisions: true },
  alias_conflict: { icon: "ArrowLeftRight", tone: "warning", bulkAccept: false, bulkReject: true, simpleDecisions: false },
  orphaned_overlay: { icon: "PencilLine", tone: "warning", bulkAccept: true, bulkReject: true, simpleDecisions: true },
  type_promotion: { icon: "PackagePlus", tone: "info", bulkAccept: false, bulkReject: true, simpleDecisions: false },
  suggestion: { icon: "MessageSquare", tone: "info", bulkAccept: true, bulkReject: true, simpleDecisions: true },
  reprocess_diff: { icon: "RefreshCw", tone: "info", bulkAccept: false, bulkReject: true, simpleDecisions: true },
  constraint_conflict: { icon: "TriangleAlert", tone: "warning", bulkAccept: false, bulkReject: false, simpleDecisions: false },
  speaker_identity: { icon: "Mic", tone: "info", bulkAccept: false, bulkReject: true, simpleDecisions: false },
};

export function presentReviewKind(kind: string): ReviewKindView {
  const known = (KINDS as Record<string, (typeof KINDS)[ReviewKindId] | undefined>)[kind];
  if (!known) return { kind: "unknown", icon: "CircleHelp", tone: "neutral", messageId: "super-drive.review.kind.unknown", bulkAccept: false, bulkReject: false, simpleDecisions: false };
  return { kind: kind as ReviewKindId, messageId: `super-drive.review.kind.${kind}`, ...known };
}

export type SourceGroup = {
  /** `null` for items that belong to no file (a type promotion, a constraint). */
  fileId: string | null;
  path: string;
  total: number;
  kinds: Record<string, number>;
};

const sourceOf = (group: ReviewGroup): { fileId: string | null; path: string } => {
  const source = (group.source ?? {}) as { file_id?: unknown; path?: unknown };
  return { fileId: typeof source.file_id === "string" ? source.file_id : null, path: typeof source.path === "string" ? source.path : "" };
};

/** One row per source file: the engine sends one group per source and kind, the queue shows the source once. */
export function aggregateGroups(groups: readonly ReviewGroup[]): SourceGroup[] {
  const bySource = new Map<string, SourceGroup>();
  for (const group of groups) {
    const { fileId, path } = sourceOf(group);
    const id = fileId ?? "";
    const entry = bySource.get(id) ?? { fileId, path, total: 0, kinds: {} };
    const count = typeof group.count === "number" ? group.count : 0;
    entry.total += count;
    if (group.kind) entry.kinds[group.kind] = (entry.kinds[group.kind] ?? 0) + count;
    bySource.set(id, entry);
  }
  return [...bySource.values()];
}

/** The kinds of a source a whole-source decision may touch, and how many items they hold. */
export function bulkScope(group: SourceGroup, decision: "accept" | "reject"): { kinds: string[]; count: number } {
  const kinds = Object.keys(group.kinds).filter((kind) => {
    const view = presentReviewKind(kind);
    return decision === "accept" ? view.bulkAccept : view.bulkReject;
  });
  return { kinds, count: kinds.reduce((sum, kind) => sum + group.kinds[kind], 0) };
}

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === "string" ? value : "");

export type EntityBrief = { key: string; name: string; type: string };
const asEntity = (value: unknown): EntityBrief | null => (isRecord(value) && typeof value.key === "string" ? { key: value.key, name: text(value.name) || value.key, type: text(value.type) } : null);

/** What a row says about its subject, by kind. Tolerates the shapes of the contract and of newer engines (unknown fields are ignored). */
export type SubjectView =
  | { kind: "fact"; subject: EntityBrief | null; predicate: string; object: EntityBrief | null; value: Record<string, unknown> | null; validFrom: string | null; validTo: string | null; factId: string | null; editableKey: string | null; editableText: string }
  | { kind: "entities"; entities: EntityBrief[]; alias: string | null }
  | { kind: "overlay"; missingEvidence: boolean }
  | { kind: "type"; label: string; instances: number; files: number; examples: string[] }
  | { kind: "constraint"; entityKey: string; factIds: string[]; constraint: string }
  | { kind: "reprocess"; prior: number; added: number; removed: number; shareUsedRemoved: number }
  | { kind: "speaker"; speaker: string; placeholder: string; candidates: { name: string; source: string; quote: string; blockId: string }[]; blobHash: string }
  | { kind: "suggestion"; suggestionId: string; shareId: string; suggestionKind: string }
  | { kind: "raw" };

const count = (value: unknown): number => (Array.isArray(value) ? value.length : typeof value === "number" ? value : 0);

export function subjectView(item: Pick<ReviewItem, "kind" | "subject">): SubjectView {
  const subject = isRecord(item.subject) ? item.subject : {};
  switch (item.kind) {
    case "low_confidence_fact": {
      const fact = isRecord(subject.fact) ? subject.fact : {};
      const value = isRecord(fact.value) ? fact.value : null;
      const editableKey = value ? Object.keys(value).find((key) => typeof value[key] === "string") ?? null : null;
      return {
        kind: "fact", subject: asEntity(fact.subject), predicate: text(fact.predicate), object: asEntity(fact.object), value,
        validFrom: typeof fact.valid_from === "string" ? fact.valid_from : null, validTo: typeof fact.valid_to === "string" ? fact.valid_to : null,
        factId: typeof fact.id === "string" ? fact.id : null, editableKey, editableText: editableKey && value ? text(value[editableKey]) : "",
      };
    }
    case "entity_merge": case "alias_conflict":
      return {
        kind: "entities",
        entities: (Array.isArray(subject.entities) ? subject.entities : []).map(asEntity).filter((entity): entity is EntityBrief => entity !== null),
        alias: typeof subject.alias === "string" ? subject.alias : null,
      };
    case "orphaned_overlay":
      return { kind: "overlay", missingEvidence: subject.missing_evidence !== false };
    case "type_promotion":
      return { kind: "type", label: text(subject.label), instances: count(subject.instances), files: count(subject.files), examples: (Array.isArray(subject.examples) ? subject.examples : []).map((example) => (isRecord(example) ? text(example.name) || text(example.text) : text(example))).filter(Boolean) };
    case "constraint_conflict":
      return { kind: "constraint", entityKey: text(subject.entity_key), factIds: (Array.isArray(subject.fact_ids) ? subject.fact_ids : []).filter((id): id is string => typeof id === "string"), constraint: text(subject.constraint) };
    case "reprocess_diff":
      return { kind: "reprocess", prior: count(subject.prior), added: count(subject.added), removed: count(subject.removed), shareUsedRemoved: count(subject.share_used_removed) };
    case "speaker_identity":
      return {
        kind: "speaker", speaker: text(subject.speaker), placeholder: text(subject.placeholder), blobHash: text(subject.b3),
        candidates: (Array.isArray(subject.candidates) ? subject.candidates : []).filter(isRecord).map((candidate) => ({ name: text(candidate.name), source: text(candidate.source), quote: text(candidate.quote), blockId: text(candidate.block_id) })).filter((candidate) => candidate.name !== ""),
      };
    case "suggestion":
      return { kind: "suggestion", suggestionId: text(subject.suggestion_id), shareId: text(subject.share_id), suggestionKind: text(subject.kind) };
    default:
      return { kind: "raw" };
  }
}

/** The five per-item outcomes of a decision, as the status line wants them (section 4.3). */
export function reviewResultParts(values: Record<string, unknown>): { messageId: string; count: number }[] {
  const num = (key: string) => (typeof values[key] === "number" ? (values[key] as number) : 0);
  const parts: { messageId: string; count: number }[] = [];
  if (num("accepted") > 0) parts.push({ messageId: "super-drive.review.result.accepted", count: num("accepted") });
  if (num("rejected") > 0) parts.push({ messageId: "super-drive.review.result.rejected", count: num("rejected") });
  if (num("alreadyDecided") > 0) parts.push({ messageId: "super-drive.review.result.alreadyDecided", count: num("alreadyDecided") });
  if (num("conflicts") > 0) parts.push({ messageId: "super-drive.review.result.conflict", count: num("conflicts") });
  if (num("notFound") > 0) parts.push({ messageId: "super-drive.review.result.notFound", count: num("notFound") });
  if (num("invalid") > 0) parts.push({ messageId: "super-drive.review.result.invalid", count: num("invalid") });
  if (parts.length === 0) parts.push({ messageId: "super-drive.review.result.nothing", count: 0 });
  return parts;
}
