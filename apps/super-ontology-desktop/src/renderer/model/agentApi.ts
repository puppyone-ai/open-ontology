import type { AgentScope, AgentTokenInfo, AgentTokenIssued } from "../../shared/super-ontology/bridge";
import { invoke, type ApiResult } from "./bridgeInvoke";

/** Agent tokens (docs-api 1.6): created once, shown once, revocable. The secret never comes back from the list. */
export const agentApi = {
  list: (): Promise<ApiResult<{ items: AgentTokenInfo[] }>> => invoke("agent-token-list", {}),
  create: (input: { name: string; scopes: AgentScope[]; driveIds?: string[]; allDrives?: boolean; expiresInSeconds?: number }): Promise<ApiResult<AgentTokenIssued>> => invoke("agent-token-create", input),
  revoke: (tokenId: string): Promise<ApiResult<{ revoked: true }>> => invoke("agent-token-revoke", { tokenId }),
};
