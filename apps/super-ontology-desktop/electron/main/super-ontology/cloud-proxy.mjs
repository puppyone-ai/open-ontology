import { SuperOntologyError } from "../../../shared/super-ontology/errors.mjs";

/**
 * This app has no hosted service: the second base of the control client is never configured, so every call stays on the
 * engine's own API (the local `oo serve` or the remote one the person connected). The shape is kept so the Share and audit
 * handlers need no branch of their own.
 */
export const CLOUD_PROXY_PATH_PREFIX = "/oo-docs";
export const CLOUD_ROUTE_NAMES = Object.freeze(["sharesCreate", "shareGet", "shareRevoke", "packCompile", "packGet"]);
export const CLOUD_MODES = Object.freeze(["local", "cloud"]);

export function createCloudProxy() {
  return Object.freeze({
    configured: () => false,
    remoteUrl: () => null,
    host: () => null,
    async request() { throw new SuperOntologyError("SO_CLOUD_NOT_CONFIGURED"); },
  });
}

/** Engine failures as Super Ontology errors; the message text of a remote party is never kept. */
export function mapCloudError(error) {
  if (error instanceof SuperOntologyError) return error;
  const status = Number.isInteger(error?.status) ? error.status : undefined;
  if (status === undefined) return new SuperOntologyError("SO_ENGINE_UNREACHABLE", { retryable: true });
  return new SuperOntologyError("SO_RESPONSE_INVALID", { httpStatus: status });
}
