import type { Alias, EntityRef, Fact } from "./contract/docsApi.generated";

/** `GET entities/{eid}` (contract `Entity`): the reference plus what the engine knows about it. */
export type EntityDetail = EntityRef & {
  status?: "active" | "merged" | "withdrawn";
  merged_into?: string | null;
  identity?: Record<string, unknown>;
  aliases?: Alias[];
  facts?: Fact[];
  history?: Fact[];
  redirect_from?: string[];
  files?: string[];
  revision?: number;
};

/** An entity search row: the reference plus the counts the engine adds. */
export type EntitySearchRow = EntityRef & { facts?: number; files?: number; aliases?: string[]; matched_alias?: string | null };

/** The pieces of a share request the dialog edits; the main process maps them to the contract. */
export type ShareSettings = {
  scope: string[];
  task: string;
  budget: number;
  role: "viewer" | "suggester" | "editor";
  expiresInSeconds: number;
  requireComplete: boolean;
  group: string;
  refreshPolicy: "manual" | "at-checkpoint" | "pinned";
  /** Rank "Other related" with the engine's local vector model (docs-api 1.5, F-S13-02); off unless chosen. */
  vector?: boolean;
};

export type ReviewDecisionOutcome = { accepted: number; rejected: number; alreadyDecided: number; conflicts: number; notFound: number; invalid: number; publishPending: boolean };
