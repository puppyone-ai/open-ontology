import type { SuperOntologyFailure } from "../../shared/super-ontology/bridge";

/** The error shape the renderer works with: an envelope `error` plus nothing else (the engine's message text never reaches us). */
export type SuperDriveError = SuperOntologyFailure["error"];

export const ERROR_FAMILIES = ["SOURCE", "PARSE", "EXTRACT", "REVIEW", "PACK", "SHARE", "SUGGEST", "ENGINE"] as const;
export type ErrorFamily = (typeof ERROR_FAMILIES)[number];

export function toSuperDriveError(failure: SuperOntologyFailure): SuperDriveError {
  const { code, retryable, details, httpStatus } = failure.error;
  return { code, retryable, details: { ...details }, ...(httpStatus === undefined ? {} : { httpStatus }) };
}

/** `DOCS_<FAMILY>_<NAME>` -> FAMILY; anything else (including `SO_*`) has none. */
export function errorFamily(code: string): ErrorFamily | null {
  const match = /^DOCS_([A-Z]+)_/u.exec(code);
  const family = match?.[1];
  return (ERROR_FAMILIES as readonly string[]).includes(family ?? "") ? (family as ErrorFamily) : null;
}

/** Message ids to try, most specific first (20-frontend §4.8): the code, its family, then the generic one. */
export function errorMessageIds(code: string): string[] {
  const family = errorFamily(code);
  return [
    `super-drive.errors.${code}`,
    ...(family ? [`super-drive.errors.family.${family}`] : []),
    "super-drive.errors.unknown",
  ];
}

/** First id that `exists`; the generic message always qualifies. */
export function resolveErrorMessageId(code: string, exists: (messageId: string) => boolean): string {
  const ids = errorMessageIds(code);
  return ids.find((id) => exists(id)) ?? ids[ids.length - 1];
}

/**
 * A write that timed out may or may not have happened. The UI says "result not
 * confirmed" and retries with the same operationId instead of sending a new
 * request (20-frontend §1.6).
 */
export function isOutcomeUncertain(error: SuperDriveError, isWrite: boolean): boolean {
  return isWrite && error.code === "SO_TIMEOUT";
}

/** Engine-side problems that mean "the engine is not there right now" rather than "this request is wrong". */
export function isEngineUnavailableError(error: SuperDriveError): boolean {
  return [
    "SO_ENGINE_NOT_RUNNING", "SO_ENGINE_UNREACHABLE", "SO_BRIDGE_UNAVAILABLE", "DOCS_ENGINE_UNAVAILABLE", "DOCS_ENGINE_STARTING",
  ].includes(error.code);
}
