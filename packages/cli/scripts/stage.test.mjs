import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { checkBinary, stage } from './stage.mjs';

function binary(target) {
  const bytes = Buffer.alloc(64);
  if (target.startsWith('linux')) {
    bytes.set([0x7f, 0x45, 0x4c, 0x46, 2, 1]);
    bytes.writeUInt16LE(target.endsWith('x64') ? 62 : 183, 18);
  } else {
    bytes.writeUInt32LE(0xfeedfacf);
    bytes.writeUInt32LE(target.endsWith('x64') ? 0x01000007 : 0x0100000c, 4);
  }
  return bytes;
}

test('native architecture is checked rather than inferred from the filename', () => {
  for (const target of ['linux-x64', 'linux-arm64', 'darwin-arm64', 'darwin-x64']) {
    assert.doesNotThrow(() => checkBinary(binary(target), target));
  }
  assert.throws(() => checkBinary(binary('linux-arm64'), 'linux-x64'));
  assert.throws(() => checkBinary(binary('darwin-arm64'), 'linux-arm64'));
  assert.throws(() => checkBinary(Buffer.from('not a binary'), 'darwin-arm64'));
});

test('stage is allowlisted, self-contained, hashed, and refuses existing output', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'oo-npm-stage-'));
  try {
    const root = join(dir, 'source');
    await mkdir(join(root, 'bin'), { recursive: true });
    await writeFile(join(root, 'package.json'), '{"name":"example"}');
    await writeFile(join(root, 'README.md'), 'usage');
    await writeFile(join(root, 'bin/oo.mjs'), '#!/usr/bin/env node\n');
    await writeFile(join(root, 'secret.token'), 'do-not-publish');
    const notices = join(dir, 'notices');
    await mkdir(notices);
    await writeFile(join(notices, 'THIRD_PARTY_NOTICES.txt'), 'Synthetic license fixture.');
    await writeFile(join(notices, 'dependencies.json'), '{"schema_version":1,"dependencies":[]}');
    const native = join(dir, 'oo');
    await writeFile(native, binary('linux-x64'));
    const output = join(dir, 'stage');
    const args = { root, output, notices, binaries: new Map([['linux-x64', native]]) };
    await assert.rejects(stage({ ...args, notices: undefined }), /dependency notices required/);
    await writeFile(join(notices, 'dependencies.json'),
      '{"schema_version":1,"dependencies":[{"unresolved":["license_text_missing"]}]}');
    await assert.rejects(stage(args), /source notice collection is incomplete/);
    await writeFile(join(notices, 'dependencies.json'), '{"schema_version":1,"dependencies":[]}');
    await stage(args);
    assert.deepEqual((await readdir(output)).sort(), [
      'LICENSE', 'NOTICE', 'README.md', 'SKILL.md', 'THIRD_PARTY_NOTICES.txt',
      'bin', 'dependencies.json', 'native', 'package.json',
    ]);
    const manifest = JSON.parse(await readFile(join(output, 'native/manifest.json'), 'utf8'));
    assert.equal(manifest.binaries['linux-x64'].path, 'native/linux-x64/oo');
    assert.match(manifest.binaries['linux-x64'].sha256, /^[a-f0-9]{64}$/);
    assert.match(manifest.skill_sha256, /^[a-f0-9]{64}$/);
    await assert.rejects(stage(args), /EEXIST/);
    await mkdir(join(dir, '.git'));
    await assert.rejects(stage({ ...args, output: join(dir, 'second') }), /outside Git/);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
