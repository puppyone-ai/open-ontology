// Runs `oo context ...` of the real engine for a T1 scenario the way an agent's setup does (the dialog itself is covered by the share scenario).
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execute = promisify(execFile);

/**
 * @param {{ environment: { ooBinary: string, environment: Record<string,string> }, stateDirectory: string }} session
 * @param {string[]} args  arguments after `oo context`
 * @param {{ cwd?: string, allowFailure?: boolean }} [options]
 * @returns {Promise<{ code: number, stdout: string, stderr: string, json: any }>}
 */
export async function ooContext(session, args, { cwd, allowFailure = false } = {}) {
  const run = execute(session.environment.ooBinary, ["context", ...args, "--state-dir", session.stateDirectory], { cwd, env: { ...process.env, ...session.environment.environment }, timeout: 120_000 });
  let result;
  try { result = { code: 0, ...(await run) }; } catch (error) {
    if (!allowFailure) throw error;
    result = { code: typeof error.code === "number" ? error.code : 1, stdout: error.stdout ?? "", stderr: error.stderr ?? "" };
  }
  let json = null;
  try { json = JSON.parse(result.stdout); } catch { /* Not JSON output. */ }
  return { ...result, json };
}

/** `oo context mount --json`; resolves to the engine's answer (it names `share_id`). */
export async function mount(session, { drive, scope, to, task, role = "suggester", budget = 800, group, expires }) {
  const args = ["mount", "--drive", drive, ...scope.flatMap((entry) => ["--scope", entry]), "--to", to, "--budget", String(budget), "--role", role, "--json"];
  if (task) args.push("--for", task);
  if (group) args.push("--group", group);
  if (expires) args.push("--expires", expires);
  return (await ooContext(session, args)).json;
}
