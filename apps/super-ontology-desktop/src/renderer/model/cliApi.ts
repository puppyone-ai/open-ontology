import type { CliError, CliResult, SuperOntologyChannel, SuperOntologyChannelMap } from "../../shared/super-ontology/bridge";
import { invoke, failure } from "./bridgeInvoke";
import { createOperationId } from "./operationIds";

/** A CLI parity call: the bridge envelope is folded into one value, so a screen only handles `CliResult` (the bridge failing is an error code too). */
export async function runCli<C extends SuperOntologyChannel>(
  channel: C, request?: SuperOntologyChannelMap[C]["request"],
): Promise<SuperOntologyChannelMap[C]["value"] extends CliResult<infer T> ? CliResult<T> : never> {
  const result = await invoke(channel, request);
  if (result.ok) return result.value as never;
  return { ok: false, exit: null, data: null, error: { code: result.error.code, message: "", hint: null }, cli: "" } as never;
}

export const withOperation = <T extends object>(prefix: string, request: T): T & { operationId: string } => ({ ...request, operationId: createOperationId(prefix) });
export { failure };

/** Codes with a written next step (`app.cli.next.<CODE>`); any other code shows the CLI's own message and hint. */
export const KNOWN_CLI_CODES = [
  "SECRET_PATH", "SECRET_OPEN", "SOURCE_TLS_CA_REQUIRED", "SO_TLS_CA_REQUIRED", "SO_SOURCE_TEST_FAILED", "SO_SOURCE_EXISTS", "SO_SOURCE_LAST",
  "SO_MODEL_REQUIRED", "SO_CHECK_NOT_SIGNED_IN", "SO_GATEWAY_NOT_CONNECTED", "READINESS_TRANSPORT", "SO_TIMEOUT", "SO_OO_MISSING", "SO_OO_INVALID",
  "ENGINE_NOT_AVAILABLE", "SO_SOURCE_SECRET_EXISTS", "SO_SOURCE_BAD_STACK", "SO_ENGINE_NOT_RUNNING", "SO_UPLOAD_CONFLICT", "SO_BRIDGE_UNAVAILABLE", "PROFILE_READ", "LITE_IMAGE_DIGEST_REQUIRED", "SECRET_FILE", "MYSQL_DOCTOR_INSTALL",
  "SO_OUTPUT_TOO_LARGE", "SO_CHECK_AFTER_ADD", "SO_CLI_FAILED", "BUILD_LOAD_FAILED", "BUILD_BUILDER_FAILED", "BUILD_EXISTS_USE_RESUME", "BUILD_LITE_STACK_CHANGED", "LITE_LIFECYCLE_FAILED", "REVIEW_FANOUT", "REVIEW_DECISION_INVALID", "REVIEW_FILE_IO", "SO_TLS_CA_NOT_ACCEPTED",
] as const;
export const hasNextStep = (error: CliError | null): boolean => error !== null && (KNOWN_CLI_CODES as readonly string[]).includes(error.code);

/** Codes whose `message` is an English sentence the main process wrote itself: it is never shown, the sentence of the code replaces it. */
export const APP_OWNED_MESSAGE_CODES: readonly string[] = [
  "SO_TIMEOUT", "SO_OUTPUT_TOO_LARGE", "SO_GATEWAY_NOT_CONNECTED", "SO_OO_INVALID", "SO_CHECK_AFTER_ADD", "SO_TLS_CA_REQUIRED", "SO_TLS_CA_NOT_ACCEPTED",
  "SO_MODEL_REQUIRED", "SO_SOURCE_SECRET_EXISTS",
];

export type CliErrorView = {
  /** Where the written-out sentence comes from: the code's next step, the bridge's own error catalog, or the generic sentence. */
  sentence: "next" | "bridge" | "generic";
  /** What the CLI itself printed (message, and the hint when no next step covers it): shown in the details line, in the CLI's own language. */
  raw: string;
};

/** Which sentence a failed command gets and what stays as raw detail. The code is always shown by the screen. */
export function describeCliError(error: CliError, bridgeKnows: (code: string) => boolean): CliErrorView {
  const next = hasNextStep(error);
  const owned = APP_OWNED_MESSAGE_CODES.includes(error.code);
  const raw = owned ? "" : [error.message, next ? "" : error.hint ?? ""].map((part) => part.trim()).filter(Boolean).join(" ");
  const sentence = next ? "next" : error.code.startsWith("SO_") && bridgeKnows(error.code) ? "bridge" : "generic";
  return { sentence, raw };
}

/** The shell refuses copies over 16 KB: bigger results are exported to a file instead. */
export const COPY_LIMIT = 16 * 1024;
