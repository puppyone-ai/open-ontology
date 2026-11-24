import type { AgentConnectSnippet } from "../../shared/super-ontology/bridge";
import { invoke, type ApiResult } from "./bridgeInvoke";

export type AgentKind = "generic" | "claude" | "cursor" | "codex";
export const AGENT_KINDS: readonly AgentKind[] = ["generic", "claude", "cursor", "codex"];

/** The connection text of an Agent, built by the main process (the shapes `oo mcp-config` prints). It never contains the token. */
export const loadSnippet = (agent: AgentKind, profile: string): Promise<ApiResult<AgentConnectSnippet>> => invoke("agent-connect-snippet", { agent, profile });
