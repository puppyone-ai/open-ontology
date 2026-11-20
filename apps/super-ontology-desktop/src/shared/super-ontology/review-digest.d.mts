export type ReviewEntity = { id: string; label: string; table: string; key: string[]; display: string | null; rows: number | null };
export type ReviewColumn = { name: string; type: string; semantic: string; pii: string };
export type ReviewDataset = { table: string; columns: ReviewColumn[] };
export type ReviewEvidence = { containment: number | null; rowsChecked: number | null; matchedRows: number | null; unmatchedRows: number | null; nameSimilarity: number | null; parentUnique: boolean; maxFanout: number | null; childDistinct: number | null; parentRows: number | null };
export type ReviewRelation = {
  key: string; ids: string[]; from: { table: string; columns: string[] }; to: { table: string; columns: string[] }; cardinality: string;
  origin: "declared" | "discovered" | "named"; defaultDecision: "adopted" | "rejected"; rule: string; reason: string; confidence: number | null; evidence: ReviewEvidence; fanOut: boolean;
};
export type ReviewDigest = {
  summaryHash: string; inputHash: string; entities: ReviewEntity[]; datasets: ReviewDataset[]; relations: ReviewRelation[];
  counts: { tables: number; columns: number; entities: number; declared: number; adopted: number; discovered: number; sensitiveHidden: number; sensitiveChecked: number };
  discovery: { status: string; checked: number; remaining: number; unchecked: number };
  builder: { declaredForeignKeys: number | null; mappedForeignKeys: number | null; mappingRate: number | null; namingConflicts: number | null; orphanRatio: number | null };
};
export type ReviewDocument = { protocol: "oo.build-review/v1"; reason: string; overrides?: { protocol: "oo.adjudication-override/v1"; summary_sha256: string; items: Record<string, { decision: "adopted" | "rejected"; reason: string }> } };
export const REVIEW_PROTOCOL: "oo.build-review/v1";
export const OVERRIDE_PROTOCOL: "oo.adjudication-override/v1";
export function fansOut(item: unknown): boolean;
export function relationOrigin(evidence: unknown): "declared" | "discovered" | "named";
export function digestReview(adjudication: unknown, reviewInput: unknown): ReviewDigest | null;
export function buildReviewDocument(adjudication: unknown, decisions: Record<string, "adopted" | "rejected">, reason: string): ReviewDocument;
