import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";
import { ensure } from "./response-guards.mjs";

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const isToken = (value) => isObject(value) && typeof value.id === "string" && typeof value.token === "string" && value.token.startsWith("ooa_");
const isList = (value) => isObject(value) && Array.isArray(value.items) && value.items.every((item) => isObject(item) && typeof item.id === "string");

/**
 * Connection text for an Agent. The engine's own `oo mcp-config` prints the same shapes; the credential is never part of them
 * (a profile holds a private reference to it). The text goes to the clipboard only when the person clicks copy.
 */
export function connectSnippet({ agent, profile }) {
  const args = ["mcp", "--stdio", "--profile", profile];
  const cli = `oo mcp-config --agent ${agent === "generic" ? "claude" : agent} --profile ${profile}`;
  if (agent === "codex") {
    return { agent, format: "toml", text: `[mcp_servers.open-ontology]\ncommand = "oo"\nargs = ${JSON.stringify(args).replace(/","/gu, '", "')}\n`, cli };
  }
  return { agent, format: "json", text: `${JSON.stringify({ mcpServers: { "open-ontology": { command: "oo", args } } }, null, 2)}\n`, cli };
}

/**
 * IPC handlers of Agent › Connect (docs-api 1.6). The `ooa_` token comes back once, to the dialog that shows it; nothing here logs
 * it, keeps it or puts it in an error.
 */
export function createAgentHandlers({ client }) {
  const call = (name, options) => client.request(name, options);
  return {
    "agent-token-create": async ({ name, scopes, driveIds, allDrives, gwProfile, label, expiresInSeconds }) => {
      if ((driveIds === undefined) === (allDrives !== true) && scopes.some((scope) => scope.startsWith("docs:"))) throw new SuperOntologyError("SO_INVALID_REQUEST");
      const body = {
        name, scopes,
        ...(driveIds === undefined ? {} : { drive_ids: driveIds }), ...(allDrives === true ? { all_drives: true } : {}),
        ...(gwProfile === undefined ? {} : { gw_profile: gwProfile }), ...(label === undefined ? {} : { label }),
        ...(expiresInSeconds === undefined ? {} : { expires_in_seconds: expiresInSeconds }),
      };
      const { value } = await call("agentTokenCreate", { body });
      if (!isToken(value)) throw new SuperOntologyError("SO_RESPONSE_INVALID");
      return value;
    },
    "agent-token-list": async () => {
      const { value } = await call("agentTokenList", {});
      return ensure(value, isList);
    },
    "agent-token-revoke": async ({ tokenId }) => {
      const { value } = await call("agentTokenRevoke", { params: { id: tokenId } });
      return { revoked: isObject(value) ? value.revoked !== false : true };
    },
    "agent-connect-snippet": async ({ agent, profile }) => connectSnippet({ agent, profile }),
  };
}
