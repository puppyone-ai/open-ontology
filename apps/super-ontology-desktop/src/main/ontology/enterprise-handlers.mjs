import { SuperOntologyError } from "../../shared/super-ontology/errors.mjs";
import { ensure } from "./response-guards.mjs";

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const isAudit = (value) => isObject(value) && Array.isArray(value.items)
  && value.items.every((item) => isObject(item) && Number.isSafeInteger(item.seq) && typeof item.at === "string" && typeof item.share_id === "string");
const isVerify = (value) => isObject(value) && typeof value.intact === "boolean";
const isTokenList = (value) => isObject(value) && Array.isArray(value.items) && value.items.every((item) => isObject(item) && typeof item.id === "string" && typeof item.name === "string");
const isToken = (value) => isObject(value) && typeof value.id === "string" && typeof value.token === "string" && value.token.startsWith("ooa_");

/**
 * IPC handlers of the audit of Shares (read-only, with its hash-chain check) and of the Agent tokens. The token comes back once, to the dialog that shows it; nothing here logs it,
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
    "agent-token-create": async ({ name, scopes, driveIds, allDrives, expiresInSeconds }) => {
      const body = {
        name, scopes,
        ...(driveIds === undefined ? {} : { drive_ids: driveIds }), ...(allDrives === undefined ? {} : { all_drives: allDrives }),
        ...(expiresInSeconds === undefined ? {} : { expires_in_seconds: expiresInSeconds }),
      };
      const { value } = await call("agentTokenCreate", { body });
      if (!isToken(value)) throw new SuperOntologyError("SO_RESPONSE_INVALID");
      return value;
    },
    "agent-token-list": async () => {
      const { value } = await call("agentTokenList", {});
      return ensure(value, isTokenList);
    },
    "agent-token-revoke": async ({ tokenId }) => {
      await call("agentTokenRevoke", { params: { id: tokenId } });
      return { revoked: true };
    },
  };
}
