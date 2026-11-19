import type {
  SuperOntologyBridge,
  SuperOntologyChannel,
  SuperOntologyChannelMap,
  SuperOntologyFailure,
  SuperOntologyResult,
} from "../../shared/super-ontology/bridge";

export type ApiResult<T> = SuperOntologyResult<T>;

let readBridgeImpl: () => SuperOntologyBridge | null = () => null;

/**
 * `superDriveApi.ts` is the only module that reads the desktop bridge global (architecture test); it installs the reader here
 * once, so the typed `invoke` below can live in a file of its own without a second place that knows the global.
 */
export function installBridgeReader(reader: () => SuperOntologyBridge | null): void {
  readBridgeImpl = reader;
}

export function readBridge(): SuperOntologyBridge | null {
  return readBridgeImpl();
}

export function failure(code: string, retryable = false): SuperOntologyFailure {
  return { ok: false, error: { code, retryable, details: {} } };
}

/** Calls the main process; failures always come back as an envelope, never as a throw. */
export async function invoke<C extends SuperOntologyChannel>(
  channel: C,
  request?: SuperOntologyChannelMap[C]["request"],
): Promise<SuperOntologyResult<SuperOntologyChannelMap[C]["value"]>> {
  const bridge = readBridge();
  if (!bridge) return failure("SO_BRIDGE_UNAVAILABLE");
  try {
    return await bridge.invoke(channel, request);
  } catch {
    return failure("SO_ENGINE_UNREACHABLE", true);
  }
}

/** Runs `guard` on a successful value; a value the app cannot interpret becomes SO_RESPONSE_INVALID. */
export function validated<T, V>(result: ApiResult<V>, guard: (value: V) => T | null): ApiResult<T> {
  if (!result.ok) return result;
  const checked = guard(result.value);
  if (checked === null) return failure("SO_RESPONSE_INVALID");
  return result.replayed ? { ok: true, value: checked, replayed: true } : { ok: true, value: checked };
}

