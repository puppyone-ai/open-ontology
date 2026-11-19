import type { Drive } from "./contract/docsApi.generated";
import type { EngineSnapshot } from "../../shared/super-ontology/bridge";

/**
 * Where the user's data actually goes (00 D79). The text on the page is
 * computed from this, never hard-coded: it follows the registered Drives, the
 * model setting, so it cannot promise something that is not
 * true (a promise that stops being true is a defect, not a copy issue).
 */
export type DataFlowInput = {
  drives: ReadonlyArray<{ backend: string; bucket?: string | null; modelEnabled?: boolean }>;
  /** `null`: not known. `configured` false: no model, the engine uses rules only. */
  model: { configured: boolean; provider?: string | null; region?: string | null } | null;
};

export type DataFlowSentence = { messageId: string; values: Record<string, string> };

export type DataFlowDestination = {
  kind: "filesLocal" | "filesBucket" | "results" | "model";
  messageId: string;
  values: Record<string, string>;
};

export type DataFlow = {
  /** Sentences of the resident line, in order. */
  summary: DataFlowSentence[];
  /** One entry per place data goes, for the full description on the engine and settings pages. */
  destinations: DataFlowDestination[];
  /** Facts the sentences were built from, for tests and the UI. */
  facts: { localFolders: number; buckets: string[]; modelProviderLabel: string | null; modelReceivesExcerpts: boolean };
};

const BUCKET_BACKENDS = new Set(["s3", "oss", "cos", "r2"]);
const CONFIGURED_MODEL_STATUSES = new Set(["ok", "degraded", "unavailable", "limited", "blocked"]);

export const joinList = (items: readonly string[]): string => items.join(", ");

function modelLabel(model: NonNullable<DataFlowInput["model"]>): string | null {
  const provider = model.provider?.trim();
  if (!provider) return null;
  const region = model.region?.trim();
  return region ? `${provider} · ${region}` : provider;
}

export function describeDataFlow(input: DataFlowInput): DataFlow {
  const buckets = [...new Set(input.drives.filter((drive) => BUCKET_BACKENDS.has(drive.backend)).map((drive) => (drive.bucket ?? "").trim() || "?"))];
  const localFolders = input.drives.filter((drive) => !BUCKET_BACKENDS.has(drive.backend)).length;
  // A configured model only sees data when at least one Drive lets it (per-Drive switch).
  const modelReceivesExcerpts = input.model?.configured === true && input.drives.some((drive) => drive.modelEnabled === true);
  const providerLabel = input.model?.configured ? modelLabel(input.model) : null;

  const summary: DataFlowSentence[] = [];
  if (buckets.length === 0) {
    summary.push({ messageId: "super-drive.common.dataLocal", values: {} });
  } else if (localFolders === 0) {
    summary.push({ messageId: "super-drive.dataFlow.summary.bucket", values: { buckets: joinList(buckets) } });
  } else {
    summary.push({ messageId: "super-drive.dataFlow.summary.mixed", values: { buckets: joinList(buckets) } });
  }
  if (modelReceivesExcerpts) {
    summary.push(providerLabel
      ? { messageId: "super-drive.dataFlow.summary.model", values: { provider: providerLabel } }
      : { messageId: "super-drive.dataFlow.summary.modelGeneric", values: {} });
  }

  const destinations: DataFlowDestination[] = [];
  if (localFolders > 0 || input.drives.length === 0) destinations.push({ kind: "filesLocal", messageId: "super-drive.dataFlow.destination.filesLocal", values: {} });
  for (const bucket of buckets) destinations.push({ kind: "filesBucket", messageId: "super-drive.dataFlow.destination.filesBucket", values: { bucket } });
  destinations.push({ kind: "results", messageId: "super-drive.dataFlow.destination.results", values: {} });
  destinations.push(modelReceivesExcerpts
    ? providerLabel
      ? { kind: "model", messageId: "super-drive.dataFlow.destination.model", values: { provider: providerLabel } }
      : { kind: "model", messageId: "super-drive.dataFlow.destination.modelGeneric", values: {} }
    : { kind: "model", messageId: "super-drive.dataFlow.destination.modelNone", values: {} });

  return { summary, destinations, facts: { localFolders, buckets, modelProviderLabel: providerLabel, modelReceivesExcerpts } };
}

/** Builds the input from what the renderer knows. The model's provider is not part of the control API yet, so it stays unnamed. */
export function dataFlowInputFromState(drives: readonly Drive[], engine: EngineSnapshot | null): DataFlowInput {
  const components = (engine?.health?.components ?? {}) as Record<string, { status?: string } | undefined>;
  const modelStatus = components.model?.status;
  return {
    drives: drives.map((drive) => ({
      backend: drive.backend,
      bucket: typeof drive.config?.bucket === "string" ? drive.config.bucket : null,
      modelEnabled: drive.model_enabled === true,
    })),
    model: modelStatus === undefined ? null : { configured: CONFIGURED_MODEL_STATUSES.has(modelStatus) },
  };
}
