import { SuperOntologyError } from "../../../shared/super-ontology/errors.mjs";

export { readTokenFile } from "./serve-credentials.mjs";

/** `http(s)://host[:port]` of an `oo serve`; credentials, paths, queries and fragments are refused. Returns the origin without a trailing slash. */
export function normalizeRemoteUrl(value) {
  let parsed;
  try { parsed = new URL(String(value ?? "")); } catch { throw new SuperOntologyError("SO_INVALID_REQUEST"); }
  if (!["http:", "https:"].includes(parsed.protocol) || parsed.username || parsed.password || parsed.search || parsed.hash
    || !["", "/"].includes(parsed.pathname) || !parsed.hostname) {
    throw new SuperOntologyError("SO_INVALID_REQUEST");
  }
  return parsed.origin;
}
