import { randomUUID } from "node:crypto";
import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";

export const GRANT_PURPOSES = Object.freeze(["drive-root", "mount-target", "oo-executable", "token-file", "ca-file", "export-dir"]);
/** Purposes whose grant may be used more than once (a dialog can retry the same target). */
const REUSABLE_PURPOSES = new Set(["mount-target"]);

/**
 * Keeps renderer-visible paths separate from the authority to act on them
 * (20-frontend §1.2.3). Host paths only enter through a native picker in the
 * main process; the renderer receives a `grantId` and a display path. Grants
 * are bound to the exact WebContents object, a new pick of the same purpose
 * replaces the old one, and (except mount-target) a grant is used once.
 */
export function createGrantStore({ createId = randomUUID } = {}) {
  const grantsBySender = new WeakMap();

  function table(sender) {
    if ((typeof sender !== "object" && typeof sender !== "function") || sender === null) {
      throw new SuperOntologyError("SO_GRANT_REQUIRED");
    }
    let grants = grantsBySender.get(sender);
    if (!grants) {
      grants = new Map();
      grantsBySender.set(sender, grants);
    }
    return grants;
  }

  function requirePurpose(purpose) {
    if (!GRANT_PURPOSES.includes(purpose)) throw new SuperOntologyError("SO_INVALID_REQUEST");
  }

  return Object.freeze({
    issue(sender, purpose, canonicalPath) {
      requirePurpose(purpose);
      if (typeof canonicalPath !== "string" || !canonicalPath) throw new SuperOntologyError("SO_INVALID_REQUEST");
      const grants = table(sender);
      const grantId = createId();
      grants.set(purpose, Object.freeze({ grantId, path: canonicalPath }));
      return Object.freeze({ grantId, displayPath: canonicalPath });
    },

    /**
     * Returns the canonical path, or throws SO_GRANT_REQUIRED for an unknown,
     * stale or wrong-purpose grant. With an `operationId` the grant stays
     * valid for retries of that same operation (a timed-out write is repeated
     * with the same id) and for nothing else.
     */
    resolve(sender, grantId, purpose, operationId = null) {
      requirePurpose(purpose);
      const grants = table(sender);
      const grant = grants.get(purpose);
      if (typeof grantId !== "string" || !grant || grant.grantId !== grantId) {
        throw new SuperOntologyError("SO_GRANT_REQUIRED");
      }
      if (REUSABLE_PURPOSES.has(purpose)) return grant.path;
      if (operationId) {
        if (grant.operationId && grant.operationId !== operationId) throw new SuperOntologyError("SO_GRANT_REQUIRED");
        grants.set(purpose, Object.freeze({ ...grant, operationId }));
        return grant.path;
      }
      grants.delete(purpose);
      return grant.path;
    },

    revoke(sender, grantId) {
      const grants = table(sender);
      for (const [purpose, grant] of grants) {
        if (grant.grantId === grantId) {
          grants.delete(purpose);
          return true;
        }
      }
      return false;
    },
  });
}
