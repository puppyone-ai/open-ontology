import { open, realpath, access } from 'node:fs/promises';
import { constants } from 'node:fs';
import { isAbsolute, normalize, basename } from 'node:path';
import { createHash } from 'node:crypto';

export const sha256 = value => createHash('sha256').update(value).digest('hex');
const sha = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const path = value => typeof value === 'string' && isAbsolute(value) &&
  normalize(value) === value && !/[\0\r\n]/.test(value);
const keys = (value, names) => value && typeof value === 'object' && !Array.isArray(value) &&
  Object.keys(value).sort().join(',') === names.split(',').sort().join(',');

// Selection is opt-in, but null, misspelled fields and incomplete pins are not opt-out.
export function skillSelection(config) {
  if (!Object.hasOwn(config, 'agentSkill')) return null;
  const skill = config.agentSkill;
  if (!keys(skill, 'source,delivery,installed,sha256,cli') ||
      !['source', 'delivery', 'installed'].every(key => path(skill[key])) ||
      !sha(skill.sha256) || !keys(skill.cli, 'declaredPath,sha256') ||
      !path(skill.cli.declaredPath) || !sha(skill.cli.sha256) ||
      !sha(config.gatewayManifestSha256)) throw new Error('INVALID_SKILL_BINDING');
  const command = config.cliCommand;
  if (!Array.isArray(command) || !command.every(arg => typeof arg === 'string')) {
    throw new Error('INVALID_SKILL_CLI_COMMAND');
  }
  if (command.length === 1 && path(command[0])) {
    return { skill, kind: 'local', actualPath: command[0] };
  }
  // Deliberately bounded: no shell, mutable container names, env flags or remote wrappers.
  if (command.length === 4 && path(command[0]) && basename(command[0]) === 'docker' &&
      command[1] === 'exec' && sha(command[2]) && path(command[3])) {
    return { skill, kind: 'docker', actualPath: command[3], prefix: command.slice(0, 3) };
  }
  throw new Error('INVALID_SKILL_CLI_COMMAND');
}

export async function fileEvidence(filename, { executable = false, text = false } = {}) {
  let handle;
  try {
    const resolved = await realpath(filename);
    handle = await open(resolved, constants.O_RDONLY | constants.O_NONBLOCK);
    const stat = await handle.stat();
    if (!stat.isFile() || !(stat.mode & 0o444) || (text && stat.size > 1048576) ||
        (executable && !(stat.mode & 0o111))) throw new Error();
    await access(resolved, executable ? constants.R_OK | constants.X_OK : constants.R_OK);
    const data = await handle.readFile();
    const after = await handle.stat();
    if (stat.size !== after.size || stat.mtimeMs !== after.mtimeMs ||
        resolved !== await realpath(filename)) throw new Error();
    const content = text ? data.toString('utf8') : undefined;
    if (text && (!content.trim() || !data.equals(Buffer.from(content, 'utf8')))) throw new Error();
    return { text: content, evidence: { path: filename, resolved_path: resolved,
      sha256: sha256(data), bytes: data.length } };
  } catch {
    throw new Error(executable ? 'CLI_BINDING_FILE_UNAVAILABLE' : 'SKILL_FILE_UNAVAILABLE');
  } finally { await handle?.close(); }
}

async function containerCall(invoke, prefix, args) {
  try {
    const result = await invoke(prefix, args);
    if (result.code !== 0 || result.limited || result.stderr) throw new Error();
    return result.stdout;
  } catch { throw new Error('SKILL_CONTAINER_BINDING_FAILED'); }
}

export async function loadSkillBinding(config, invoke) {
  const selected = skillSelection(config);
  if (!selected) return null;
  const { skill, kind, actualPath, prefix } = selected;
  const source = await fileEvidence(skill.source, { text: true });
  const delivery = await fileEvidence(skill.delivery, { text: true });
  let installed, cli, launcher;
  if (kind === 'local') {
    installed = await fileEvidence(skill.installed, { text: true });
    cli = (await fileEvidence(actualPath, { executable: true })).evidence;
  } else {
    launcher = (await fileEvidence(prefix[0], { executable: true })).evidence;
    for (const [flag, filename] of [
      ['-f', skill.installed], ['-r', skill.installed],
      ['-f', actualPath], ['-r', actualPath], ['-x', actualPath],
    ]) await containerCall(invoke, prefix, ['test', flag, filename]);
    const text = await containerCall(invoke, prefix, ['cat', '--', skill.installed]);
    if (!text.trim() || Buffer.byteLength(text) > 1048576) throw new Error('SKILL_FILE_UNAVAILABLE');
    installed = { text, evidence: { path: skill.installed, sha256: sha256(text),
      bytes: Buffer.byteLength(text) } };
    const sum = await containerCall(invoke, prefix, ['sha256sum', '--', actualPath]);
    if (!/^[a-f0-9]{64}  /.test(sum) || sum.slice(66).trimEnd() !== actualPath) {
      throw new Error('SKILL_CONTAINER_BINDING_FAILED');
    }
    cli = { path: actualPath, sha256: sum.slice(0, 64) };
  }
  if ([source, delivery, installed].some(file => file.evidence.sha256 !== skill.sha256)) {
    throw new Error('SKILL_SHA_DRIFT');
  }
  if (cli.sha256 !== skill.cli.sha256) throw new Error('CLI_BINDING_SHA_DRIFT');
  return { text: installed.text, evidence: {
    source: source.evidence, delivery: delivery.evidence, installed: installed.evidence,
    cli: { kind, declared_path: skill.cli.declaredPath, actual: cli,
      command_sha256: sha256(JSON.stringify(config.cliCommand)),
      ...(prefix ? { container_id: prefix[2], launcher } : {}) },
  } };
}
