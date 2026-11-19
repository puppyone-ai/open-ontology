import { SuperOntologyError } from "../../../shared/super-ontology/errors.mjs";

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

/**
 * Structural checks of the key fields of engine responses (20-frontend
 * §1.4.2 "响应校验"). Unknown enum values and extra fields are accepted on
 * purpose: the engine may be newer than the app (10 §8.1).
 */
/** A list without the `as_of_seq` the contract promises: the engine's share lists do not carry it (reported to the OO side). */
export const isItemPage = (value) => isObject(value) && Array.isArray(value.items) && (value.next_cursor === null || value.next_cursor === undefined || typeof value.next_cursor === "string");

export const isPage = (value) => isObject(value) && Array.isArray(value.items)
  && (value.next_cursor === null || typeof value.next_cursor === "string")
  && Number.isSafeInteger(value.as_of_seq);

export const isDrive = (value) => isObject(value) && typeof value.id === "string" && typeof value.name === "string"
  && typeof value.backend === "string" && typeof value.status === "string" && Number.isSafeInteger(value.revision);

export const isTree = (value) => isObject(value) && Array.isArray(value.dirs)
  && value.dirs.every((dir) => isObject(dir) && typeof dir.path === "string") && isPage(value.files);

export const isSyncInfo = (value) => value === null || isObject(value);

export const isFileSummary = (value) => isObject(value) && typeof value.id === "string" && typeof value.path === "string"
  && typeof value.state === "string";

export const isBatchResult = (value) => isObject(value) && Array.isArray(value.results)
  && value.results.every((entry) => isObject(entry) && typeof entry.id === "string" && typeof entry.status === "string");

/** Returns `value` or throws SO_RESPONSE_INVALID. */
export function ensure(value, guard) {
  if (!guard(value)) throw new SuperOntologyError("SO_RESPONSE_INVALID");
  return value;
}
