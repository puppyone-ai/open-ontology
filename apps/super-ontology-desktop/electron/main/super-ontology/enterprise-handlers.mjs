import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";
import { ensure } from "./response-guards.mjs";

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const isAudit = (value) => isObject(value) && Array.isArray(value.items)
  && value.items.every((item) => isObject(item) && Number.isSafeInteger(item.seq) && typeof item.at === "string" && typeof item.share_id === "string");
const isVerify = (value) => isObject(value) && typeof value.intact === "boolean";
const isToken = (value) => isObject(value) && typeof value.id === "string" && typeof value.token === "string" && value.token.startsWith("oodc_");

/**
 * IPC handlers of the L7 channels that are not about the mode itself (F-S12-02): the read-only audit of Shares, its hash-chain
 * check and the tokens of the engine's cloud entry. The token comes back once, to the dialog that shows it; nothing here logs it,
 * keeps it or puts it in an error (an engine message never reaches the renderer either).
 */
export function createEnterpriseHandlers({ client }) {
  const call = (name, options) => client.request(name, options);
  return {
    "audit-shares": async ({ driveId, agent, tenant, since, until }) => {
      const { value } = await call("auditShares", { query: { drive_id: driveId, agent, tenant, since, until } });
      return ensure(value, isAudit);
    },
    "audit-verify": async () => {
      const { value } = await call("auditVerify", {});
      return ensure(value, isVerify);
    },
    "cloud-token-create": async ({ tenant, driveIds, label, expiresInSeconds }) => {
      const body = {
        tenant, drive_ids: driveIds,
        ...(label === undefined ? {} : { label }), ...(expiresInSeconds === undefined ? {} : { expires_in_seconds: expiresInSeconds }),
      };
      const { value } = await call("cloudTokenCreate", { body });
      if (!isToken(value)) throw new SuperOntologyError("SO_RESPONSE_INVALID");
      return value;
    },
    "cloud-token-revoke": async ({ tokenId }) => {
      await call("cloudTokenRevoke", { params: { id: tokenId } });
      return { revoked: true };
    },
  };
}
