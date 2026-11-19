import type {
  CompileInput,
  ModelConfigSaveInput,
  ReviewBySource,
  ReviewDecisionItem,
  ReviewDecisionResult,
} from "../../shared/super-ontology/bridge";
import { invoke, validated, type ApiResult } from "./bridgeInvoke";
import {
  isBatchResult,
  isConstraint,
  isContextPack,
  isFact,
  isModelConfig,
  isOntologyVersion,
  isReviewGroup,
  isReviewItem,
  isRevokeResult,
  isShare,
  isShareUpdate,
  isUsage,
  type BatchResult,
  type Constraint,
  type ContextPack,
  type Fact,
  type ModelConfig,
  type OntologyVersion,
  type Page,
  type ResolveResult,
  type ReviewGroup,
  type ReviewItem,
  type RevokeResult,
  type Share,
  type ShareUpdate,
  type Usage,
} from "./contract/docsApi.generated";
import type { EntityDetail, EntitySearchRow } from "./superDriveOntologyTypes";

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const isPage = (value: unknown): value is Page<unknown> => isRecord(value) && Array.isArray(value.items)
  && (value.next_cursor === null || typeof value.next_cursor === "string") && Number.isSafeInteger(value.as_of_seq);
/** A page whose items all pass `guard`; engines may leave `as_of_seq` out of lists that have no snapshot (versions, constraints). */
const pageOf = <T,>(guard: (item: unknown) => boolean) => (value: unknown): Page<T> | null => {
  if (!isRecord(value) || !Array.isArray(value.items) || !(value.next_cursor === null || value.next_cursor === undefined || typeof value.next_cursor === "string")) return null;
  if (!value.items.every(guard)) return null;
  return { items: value.items as T[], next_cursor: (value.next_cursor as string | null | undefined) ?? null, as_of_seq: Number.isSafeInteger(value.as_of_seq) ? (value.as_of_seq as number) : 0 };
};
const isEntityRow = (item: unknown) => isRecord(item) && typeof item.id === "string" && typeof item.name === "string" && typeof item.key === "string";
const isEntityDetail = (value: unknown): EntityDetail | null => (isEntityRow(value) ? (value as EntityDetail) : null);
const isResolve = (value: unknown): ResolveResult | null => (isRecord(value) ? (value as ResolveResult) : null);
const isDecision = (value: unknown): ReviewDecisionResult | null => (isRecord(value) ? (value as ReviewDecisionResult) : null);
const asShare = (value: unknown): Share | null => (isShare(value) ? value : null);
const asPack = (value: unknown): ContextPack | null => (isContextPack(value) ? value : null);

/** The L4 and L6 part of what the store depends on; tests pass a fake with the same shape. */
export type OntologyApi = {
  searchEntities(input: { q: string; driveId?: string; type?: string; limit?: number; cursor?: string }): Promise<ApiResult<Page<EntitySearchRow>>>;
  entityDetail(input: { entityIdOrKey: string; includeHistory?: boolean }): Promise<ApiResult<EntityDetail>>;
  resolveText(input: { text: string; driveId?: string; types?: string[]; limit?: number }): Promise<ApiResult<ResolveResult>>;
  reviewGroups(input: { driveId: string; kind?: string; limit?: number; cursor?: string }): Promise<ApiResult<Page<ReviewGroup>>>;
  reviewItems(input: { driveId: string; kind?: string; source?: string; status?: string; limit?: number; cursor?: string }): Promise<ApiResult<Page<ReviewItem>>>;
  decideReview(input: { operationId: string; items?: ReviewDecisionItem[]; bySource?: ReviewBySource }): Promise<ApiResult<ReviewDecisionResult>>;
  compilePack(input: { operationId: string; compile: CompileInput }): Promise<ApiResult<ContextPack>>;
  readPack(input: { packId: string }): Promise<ApiResult<ContextPack>>;
  listShares(input: { driveId?: string; states?: string[]; limit?: number; cursor?: string }): Promise<ApiResult<Page<Share>>>;
  getShare(input: { shareId: string }): Promise<ApiResult<Share>>;
  mountShare(input: { operationId: string; compile: CompileInput; targetGrantId: string; expiresInSeconds: number; group?: string; refreshPolicy?: "manual" | "at-checkpoint" | "pinned" }): Promise<ApiResult<Share>>;
  refreshShare(input: { operationId: string; shareId: string }): Promise<ApiResult<Share>>;
  revokeShare(input: { operationId: string; shareId: string; reason?: string }): Promise<ApiResult<RevokeResult>>;
  revealShare(input: { shareId: string }): Promise<ApiResult<{ revealed: boolean }>>;
  shareUpdates(input: { shareId: string; limit?: number; cursor?: string }): Promise<ApiResult<Page<ShareUpdate>>>;
  refreshShareGroup(input: { operationId: string; groupId: string }): Promise<ApiResult<BatchResult>>;
  readUsage(input: { driveId?: string; days?: number }): Promise<ApiResult<Usage>>;
  ontologyVersions(input: { limit?: number; cursor?: string }): Promise<ApiResult<Page<OntologyVersion>>>;
  readModelConfig(): Promise<ApiResult<ModelConfig>>;
  saveModelConfig(input: ModelConfigSaveInput): Promise<ApiResult<ModelConfig>>;
  correctFact(input: { operationId: string; factId: string; revision: number; action: "mark_wrong" | "set_value"; value?: Record<string, unknown>; reason?: string }): Promise<ApiResult<Fact>>;
  mergeEntities(input: { operationId: string; into: string; from: string[]; revision: number }): Promise<ApiResult<EntityDetail>>;
  splitEntity(input: { operationId: string; entityId: string; revision: number; groups: { aliases?: string[]; facts?: string[] }[] }): Promise<ApiResult<BatchResult>>;
  listConstraints(input?: { limit?: number; cursor?: string }): Promise<ApiResult<Page<Constraint>>>;
  /** Decodes a recording on this computer (macOS) into the engine's exchange directory; the engine then parses it. */
  decodeMedia(input: { fileId: string }): Promise<ApiResult<{ decoded: true; durationMs: number; frames: number; hasAudio: boolean; still: boolean; helperVersion: string }>>;
  cancelMediaDecode(input: { fileId: string }): Promise<ApiResult<{ cancelled: boolean }>>;
};

export const ontologyApi: OntologyApi = {
  searchEntities: async (input) => validated(await invoke("entities-search", input), pageOf<EntitySearchRow>(isEntityRow)),
  entityDetail: async (input) => validated(await invoke("entity-detail", input), isEntityDetail),
  resolveText: async (input) => validated(await invoke("resolve", input), isResolve),
  reviewGroups: async (input) => validated(await invoke("review-list", { ...input, groupBy: "source", status: "pending" }), pageOf<ReviewGroup>(isReviewGroup)),
  reviewItems: async (input) => validated(await invoke("review-list", input), pageOf<ReviewItem>(isReviewItem)),
  decideReview: async (input) => validated(await invoke("review-decide", input), isDecision),
  compilePack: async (input) => validated(await invoke("pack-compile", input), asPack),
  readPack: async (input) => validated(await invoke("pack-read", input), asPack),
  listShares: async (input) => validated(await invoke("shares-list", input), pageOf<Share>(isShare)),
  getShare: async (input) => validated(await invoke("share-get", input), asShare),
  mountShare: async (input) => validated(await invoke("share-mount", input), asShare),
  refreshShare: async (input) => validated(await invoke("share-refresh", input), asShare),
  revokeShare: async (input) => validated(await invoke("share-revoke", input), (value) => (isRevokeResult(value) ? value : null)),
  revealShare: (input) => invoke("share-reveal", input),
  shareUpdates: async (input) => validated(await invoke("share-updates", input), pageOf<ShareUpdate>(isShareUpdate)),
  refreshShareGroup: async (input) => validated(await invoke("share-group-refresh", input), (value) => (isBatchResult(value) ? value : null)),
  readUsage: async (input) => validated(await invoke("usage-read", input), (value) => (isUsage(value) ? value : null)),
  ontologyVersions: async (input) => validated(await invoke("ontology-versions", input), pageOf<OntologyVersion>((item) => isOntologyVersion(item))),
  readModelConfig: async () => validated(await invoke("model-config-read", {}), (value) => (isModelConfig(value) ? value : null)),
  saveModelConfig: async (input) => validated(await invoke("model-config-save", input), (value) => (isModelConfig(value) ? value : null)),
  correctFact: async (input) => validated(await invoke("fact-correct", input), (value) => (isFact(value) ? value : null)),
  mergeEntities: async (input) => validated(await invoke("entity-merge", input), isEntityDetail),
  splitEntity: async (input) => validated(await invoke("entity-split", input), (value) => (isBatchResult(value) ? value : null)),
  decodeMedia: (input) => invoke("media-decode", input),
  cancelMediaDecode: (input) => invoke("media-decode-cancel", input),
  listConstraints: async (input = {}) => validated(await invoke("constraints-list", input), pageOf<Constraint>((item) => isConstraint(item))),
};

export { isPage as isEnginePage };
