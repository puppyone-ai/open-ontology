import type { SuperOntologyFailure } from "./bridge";
export const SO_ERROR_CODES: readonly string[];
export class SuperOntologyError extends Error {
  constructor(code: string, options?: { retryable?: boolean; details?: Record<string, unknown>; httpStatus?: number });
  code: string;
  retryable: boolean;
  details: Record<string, unknown>;
  httpStatus?: number;
}
export function invalidRequest(): SuperOntologyError;
export function toErrorResult(error: unknown): SuperOntologyFailure;
export function pickDetails(code: string, details: unknown): Record<string, unknown>;
