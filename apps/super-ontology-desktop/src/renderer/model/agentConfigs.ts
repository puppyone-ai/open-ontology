export type AgentKind = "mcp" | "claude" | "cursor" | "codex";
export const AGENT_KINDS: readonly AgentKind[] = ["mcp", "claude", "cursor", "codex"];

export type ConnectInput = { endpoint: string; tokenFile: string };

const quote = (value: string) => (/^[A-Za-z0-9_./:@%+=,-]+$/u.test(value) ? value : `'${value.replace(/'/gu, `'\\''`)}'`);
const mcpArgs = ({ endpoint, tokenFile }: ConnectInput) => ["mcp", "--stdio", "--endpoint", endpoint, "--token-file", tokenFile];

/** What each Agent is given. The secret is never inside: the config names the owner-only file that holds it. */
export function buildAgentConfig(kind: AgentKind, input: ConnectInput): { language: "json" | "toml" | "shell"; text: string; where: "generic" | "claude" | "cursor" | "codex" } {
  const args = mcpArgs(input);
  if (kind === "codex") {
    return { language: "toml", where: "codex", text: `[mcp_servers.super-ontology]\ncommand = "oo"\nargs = [${args.map((a) => JSON.stringify(a)).join(", ")}]\n` };
  }
  if (kind === "claude") {
    return { language: "shell", where: "claude", text: `claude mcp add super-ontology -- oo ${args.map(quote).join(" ")}` };
  }
  const list = args.map((a) => JSON.stringify(a)).join(", ");
  return { language: "json", where: kind === "cursor" ? "cursor" : "generic", text: `{\n  "mcpServers": {\n    "super-ontology": {\n      "command": "oo",\n      "args": [${list}]\n    }\n  }\n}\n` };
}

/** The same query from a terminal, no agent needed. */
export function buildCliCheck({ endpoint, tokenFile }: ConnectInput): string {
  return `oo ontology search "your question" --endpoint ${quote(endpoint)} --token-file ${quote(tokenFile)}`;
}

/** The command that issues the same token without the app. */
export function buildTokenCommand(input: { name: string; allDrives: boolean; driveNames: string[]; ttlDays: number }): string {
  const drives = input.allDrives ? "--all-drives" : input.driveNames.map((name) => `--drive ${quote(name)}`).join(" ");
  return `oo agent token create --name ${quote(input.name)} --scope docs:read ${drives} --ttl ${input.ttlDays}d --output-file ~/.config/oo/${input.name}.token`;
}
