import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, chmod, rm, symlink, readFile, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { loadSkillBinding, sha256, skillSelection } from '../agent-contract/skill-binding.mjs';
import { prepareEvaluation, verifyWirePrompt } from './agent.mjs';

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'selected-skill-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const text = '---\nname: fixture-business\n---\n# Private fixture\nQuery before answering. Never turn errors into zero.\n';
  for (const file of ['source.md', 'delivery.md', 'installed.md']) await writeFile(join(root, file), text);
  const cli = join(root, 'oo');
  await writeFile(cli, '#!/bin/sh\nexit 0\n', { mode: 0o700 });
  const config = { cliCommand: [cli], gatewayManifestSha256: 'a'.repeat(64),
    agentSkill: { source: join(root, 'source.md'), delivery: join(root, 'delivery.md'),
      installed: join(root, 'installed.md'), sha256: sha256(text),
      cli: { declaredPath: '/opt/customer/bin/oo', sha256: sha256(await readFile(cli)) } } };
  return { root, config, text, cli };
}

test('explicit installed Skill is verbatim in actual prepared prompt, with only hash metadata emitted', async t => {
  const { config, text } = await fixture(t);
  const prepared = await prepareEvaluation(config);
  assert(prepared.systemPrompt.endsWith(text));
  assert(prepared.systemPrompt.includes('query_ids'));
  assert(prepared.systemPrompt.includes('rows'));
  assert(prepared.systemPrompt.includes(config.cliCommand[0]));
  assert(prepared.systemPrompt.includes(config.agentSkill.cli.declaredPath));
  assert.equal(prepared.evidence.system_prompt_sha256, sha256(prepared.systemPrompt));
  for (const stage of ['source', 'delivery', 'installed']) {
    assert.equal(prepared.evidence.skill[stage].sha256, sha256(text));
  }
  assert.equal(prepared.evidence.skill.cli.actual.sha256, config.agentSkill.cli.sha256);
  assert(!JSON.stringify(prepared.evidence).includes('Private fixture'));
  assert.equal(verifyWirePrompt({ messages: [{ role: 'system', content: prepared.systemPrompt }] }, prepared),
    prepared.evidence.system_prompt_sha256);
  for (const messages of [[], [{ role: 'system', content: 'different' }],
    [{ role: 'user', content: prepared.systemPrompt }]]) {
    assert.throws(() => verifyWirePrompt({ messages }, prepared), /PROMPT_BINDING_DRIFT/);
  }
});

test('generic preparation needs no private customer paths or pinned CLI', async () => {
  const prepared = await prepareEvaluation({});
  assert.equal(prepared.evidence.skill, null);
  assert(prepared.systemPrompt.includes('query_ids'));
  assert(!prepared.systemPrompt.includes('/opt/customer'));
});

test('explicit selection never silently falls back on malformed config or unpinned runtime', async t => {
  const { config } = await fixture(t);
  for (const agentSkill of [null, false, '', {}, { ...config.agentSkill, sha256: 'bad' },
    { ...config.agentSkill, installed: 'relative.md' },
    { ...config.agentSkill, source: '/private/../secret' },
    { ...config.agentSkill, optional: true }]) {
    await assert.rejects(prepareEvaluation({ ...config, agentSkill }), /INVALID_SKILL_BINDING/);
  }
  await assert.rejects(prepareEvaluation({ ...config, gatewayManifestSha256: undefined }), /INVALID_SKILL_BINDING/);
  for (const cliCommand of [['oo'], ['sh', '-c', '/private/oo'], [config.cliCommand[0], '--token=secret'],
    ['/usr/bin/docker', 'exec', 'mutable-name', '/opt/oo']]) {
    assert.throws(() => skillSelection({ ...config, cliCommand }), /INVALID_SKILL_CLI_COMMAND/);
  }
});

for (const stage of ['source', 'delivery', 'installed']) {
  test(`${stage} missing, unreadable, wrong path, or SHA drift is rejected`, async t => {
    const { config, root, text } = await fixture(t);
    await assert.rejects(prepareEvaluation({ ...config, agentSkill: {
      ...config.agentSkill, [stage]: join(root, 'missing'),
    } }), /SKILL_FILE_UNAVAILABLE/);
    await assert.rejects(prepareEvaluation({ ...config, agentSkill: {
      ...config.agentSkill, [stage]: root,
    } }), /SKILL_FILE_UNAVAILABLE/);
    await chmod(config.agentSkill[stage], 0);
    await assert.rejects(prepareEvaluation(config), /SKILL_FILE_UNAVAILABLE/);
    await chmod(config.agentSkill[stage], 0o600);
    await writeFile(config.agentSkill[stage], text + 'changed\n');
    await assert.rejects(prepareEvaluation(config), /SKILL_SHA_DRIFT/);
  });
}

test('changed selected bytes with refreshed pins still invalidate old prompt evidence', async t => {
  const { config, text } = await fixture(t);
  const before = await prepareEvaluation(config);
  const changed = text + 'New approved instruction.\n';
  for (const stage of ['source', 'delivery', 'installed']) await writeFile(config.agentSkill[stage], changed);
  config.agentSkill.sha256 = sha256(changed);
  const after = await prepareEvaluation(config);
  assert.notEqual(before.evidence.system_prompt_sha256, after.evidence.system_prompt_sha256);
});

test('actual CLI bytes and executable permissions are checked, including symlink target', async t => {
  const { config, cli, root } = await fixture(t);
  const link = join(root, 'linked-oo');
  await symlink(cli, link);
  const linked = await loadSkillBinding({ ...config, cliCommand: [link] });
  assert.equal(linked.evidence.cli.actual.resolved_path, await realpath(cli));
  await chmod(cli, 0o600);
  await assert.rejects(prepareEvaluation(config), /CLI_BINDING_FILE_UNAVAILABLE/);
  await chmod(cli, 0o700);
  await writeFile(cli, '#!/bin/sh\nexit 2\n');
  await assert.rejects(prepareEvaluation(config), /CLI_BINDING_SHA_DRIFT/);
});

test('fixed container binding inspects installed Skill and the CLI inside that exact runtime', async t => {
  const { config, root, text } = await fixture(t);
  const launcher = join(root, 'docker');
  await writeFile(launcher, '#!/bin/sh\nexit 99\n', { mode: 0o700 });
  config.cliCommand = [launcher, 'exec', 'b'.repeat(64), '/opt/runtime/oo'];
  config.agentSkill.installed = '/opt/private/SKILL.md';
  const calls = [];
  const invoke = async (prefix, args) => {
    calls.push(args);
    assert.deepEqual(prefix, config.cliCommand.slice(0, 3));
    return { code: 0, stdout: args[0] === 'cat' ? text
      : args[0] === 'sha256sum' ? config.agentSkill.cli.sha256 + '  /opt/runtime/oo\n' : '' };
  };
  const binding = await loadSkillBinding(config, invoke);
  assert.equal(binding.evidence.cli.kind, 'docker');
  assert.equal(binding.evidence.cli.container_id, 'b'.repeat(64));
  assert.equal(binding.evidence.cli.actual.path, '/opt/runtime/oo');
  assert(calls.some(args => args.join(' ') === 'test -x /opt/runtime/oo'));
  assert(calls.some(args => args.join(' ') === 'cat -- /opt/private/SKILL.md'));
  await assert.rejects(loadSkillBinding(config, async () => ({ code: 1, stderr: 'private secret' })),
    /^Error: SKILL_CONTAINER_BINDING_FAILED$/);
  await assert.rejects(loadSkillBinding(config, async (prefix, args) => args[0] === 'sha256sum'
    ? { code: 0, stdout: '0'.repeat(64) + '  /opt/runtime/oo\n' } : invoke(prefix, args)),
  /CLI_BINDING_SHA_DRIFT/);
});

test('inspection and pre-model rejection are real entrypoint callers, never import SDK or load credentials', async t => {
  const { config } = await fixture(t);
  config.demoDir = '/not-installed';
  config.bearerFile = '/do-not-read';
  const entry = fileURLToPath(new URL('./agent.mjs', import.meta.url));
  const inspect = spawnSync(process.execPath, [entry, '--inspect-binding'],
    { input: JSON.stringify({ config }), encoding: 'utf8' });
  assert.equal(inspect.status, 0);
  assert.equal(JSON.parse(inspect.stdout).skill.installed.sha256, config.agentSkill.sha256);
  config.agentSkill.sha256 = '0'.repeat(64);
  const run = spawnSync(process.execPath, [entry],
    { input: JSON.stringify({ config, text: 'Do not call any model' }), encoding: 'utf8' });
  assert.equal(run.status, 2);
  assert.deepEqual(JSON.parse(run.stdout), { trace: [], error: 'SKILL_SHA_DRIFT' });
  assert.equal(run.stderr, '');
});
