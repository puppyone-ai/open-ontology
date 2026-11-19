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
  "ENGINE_NOT_AVAILABLE", "SO_SOURCE_SECRET_EXISTS", "SO_SOURCE_BAD_STACK", "SO_ENGINE_NOT_RUNNING", "SO_UPLOAD_CONFLICT", "SO_BRIDGE_UNAVAILABLE", "PROFILE_READ",
] as const;
export const hasNextStep = (error: CliError | null): boolean => error !== null && (KNOWN_CLI_CODES as readonly string[]).includes(error.code);

/** The shell refuses copies over 16 KB: bigger results are exported to a file instead. */
export const COPY_LIMIT = 16 * 1024;
