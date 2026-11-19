// Agents: create a read-only token (the secret is shown once), copy-ready connection text per agent kind without the secret, revoke.
import { runScenario } from "../harness/session.mjs";

runScenario({ name: "agents", title: "Agents: token, connection text, revoke" }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.openSeeded("agents");
  await s.waitFor("screen-agents");

  await s.click("agent-token-create");
  await s.type("agent-token-name", "t1-reader");
  await s.click("agent-token-submit");
  await s.waitFor("agent-token-secret");
  const secret = (await s.text("agent-token-secret")).trim();
  s.check("the token secret is shown once", /^ooa_/u.test(secret));
  await s.waitFor("agent-token-list");
  await s.waitFor("agent-token-row", { "data-name": "t1-reader" });

  await s.click("agent-kind-claude");
  await s.waitFor("agent-config-text");
  const claude = await s.text("agent-config-text");
  s.check("the Claude text is an MCP server entry for open-ontology", /open-ontology/u.test(claude) && /mcp/u.test(claude));
  s.check("the connection text carries no credential", !claude.includes(secret));
  await s.click("agent-kind-codex");
  await s.until("/mcp_servers/.test(document.querySelector('[data-testid=\"agent-config-text\"]')?.innerText ?? '')", "the Codex text");
  s.check("the Codex text is TOML", true);

  const listed = await s.engineApi("/v1/docs/agent-tokens");
  s.check("the engine knows the token", (listed.body?.items ?? []).some((token) => token.name === "t1-reader"), JSON.stringify((listed.body?.items ?? []).map((token) => token.name)));
  await s.click("agent-token-revoke", {});
  if (await s.exists("dialog-confirm")) await s.click("dialog-confirm");
  await s.waitFor("agent-token-row", { "data-name": "t1-reader", "data-state": "revoked" });
  s.check("a revoked token is shown as revoked", true);
  await s.shot("agents");
});
