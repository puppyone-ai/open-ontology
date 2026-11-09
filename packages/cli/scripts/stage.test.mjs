import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { checkBinary, checkImage, stage } from './stage.mjs';

test('lite image sidecar only accepts pinned references', () => {
  for (const ref of [`sha256:${'a'.repeat(64)}`, `registry.example/oo@sha256:${'b'.repeat(64)}`]) {
    assert.equal(checkImage(ref), ref + '\n');
  }
  for (const ref of ['', 'latest', 'repo:tag', 'sha256:abc', `repo;command@sha256:${'a'.repeat(64)}`]) {
    assert.throws(() => checkImage(ref), /pinned content digest/);
  }
});

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
    await writeFile(join(root, 'package.json'),
      await readFile(new URL('../package.json', import.meta.url)));
    await writeFile(join(root, 'README.md'), 'usage');
    await writeFile(join(root, 'bin/oo.mjs'), '#!/usr/bin/env node\n');
    await writeFile(join(root, 'bin/auth.mjs'), '// auth module\n');
    await writeFile(join(root, 'secret.token'), 'do-not-publish');
    const notices = join(dir, 'notices');
    await mkdir(notices);
    await writeFile(join(notices, 'THIRD_PARTY_NOTICES.txt'), 'Synthetic license fixture.');
    await writeFile(join(notices, 'dependencies.json'), '{"schema_version":1,"dependencies":[]}');
    const native = join(dir, 'oo');
    await writeFile(native, binary('linux-x64'));
    const output = join(dir, 'stage');
    const args = { root, output, notices, binaries: new Map([['linux-x64', native]]), partial: true };
    await assert.rejects(stage({ ...args, partial: false }), /every declared platform/);
    await assert.rejects(stage({ ...args, notices: undefined }), /dependency notices required/);
    await writeFile(join(notices, 'dependencies.json'),
      '{"schema_version":1,"dependencies":[{"unresolved":["license_text_missing"]}]}');
    await assert.rejects(stage(args), /source notice collection is incomplete/);
    await writeFile(join(notices, 'dependencies.json'), '{"schema_version":1,"dependencies":[]}');
    await assert.rejects(stage(args), /ENOENT/);
    const image = `registry.example/oo@sha256:${'a'.repeat(64)}\n`;
    await writeFile(join(dir, 'oo-lite-image'), image);
    await writeFile(join(dir, 'oo-mysql-doctor'), binary('linux-x64'));
    await stage(args);
    const metadata = JSON.parse(await readFile(join(output, 'package.json'), 'utf8'));
    assert.equal(metadata.private, true);
    assert.deepEqual(metadata.ooPlatforms, ['linux-x64']);
    assert.deepEqual((await readdir(output)).sort(), [
      'LICENSE', 'NOTICE', 'README.md', 'SKILL.md', 'THIRD_PARTY_NOTICES.txt',
      'bin', 'dependencies.json', 'native', 'package.json',
    ]);
    const manifest = JSON.parse(await readFile(join(output, 'native/manifest.json'), 'utf8'));
    assert.equal(manifest.binaries['linux-x64'].path, 'native/linux-x64/oo');
    assert.match(manifest.binaries['linux-x64'].sha256, /^[a-f0-9]{64}$/);
    assert.match(manifest.binaries['linux-x64'].mysql_doctor.sha256, /^[a-f0-9]{64}$/);
    assert.match(manifest.skill_sha256, /^[a-f0-9]{64}$/);
    assert.equal(await readFile(join(output, 'native/linux-x64/oo-lite-image'), 'utf8'), image);
    const packed = JSON.parse(execFileSync('npm', ['pack', '--json', '--ignore-scripts',
      '--pack-destination', dir], { cwd: output, encoding: 'utf8',
      env: { ...process.env, npm_config_cache: join(dir, 'npm-cache') } }))[0];
    const files = new Set(packed.files.map(file => file.path));
    for (const file of ['native/linux-x64/oo-lite-image', 'native/linux-x64/oo-mysql-doctor', 'NOTICE',
      'THIRD_PARTY_NOTICES.txt', 'dependencies.json']) assert.ok(files.has(file), file);
    assert.ok(!files.has('secret.token'));
    await assert.rejects(stage(args), /EEXIST/);
    await mkdir(join(dir, '.git'));
    await assert.rejects(stage({ ...args, output: join(dir, 'second') }), /outside Git/);
    execFileSync('git', ['init', '--quiet', dir]);
    await writeFile(join(dir, '.gitignore'), '/ignored/\n');
    await mkdir(join(dir, 'ignored'));
    await stage({ ...args, output: join(dir, 'ignored', 'package') });
    await assert.rejects(stage({ ...args, output: join(dir, 'tracked') }), /outside Git/);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test('release tarball contains every declared native binary and sidecar', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'oo-release-stage-'));
  try {
    const metadata = JSON.parse(await readFile(new URL('../package.json', import.meta.url)));
    const notices = join(dir, 'notices');
    await mkdir(notices);
    await writeFile(join(notices, 'THIRD_PARTY_NOTICES.txt'), 'Synthetic fixture.');
    await writeFile(join(notices, 'dependencies.json'), '{"schema_version":1,"dependencies":[]}');
    const binaries = new Map();
    for (const target of metadata.ooPlatforms) {
      const parent = join(dir, target);
      await mkdir(parent);
      await writeFile(join(parent, 'oo'), binary(target));
      await writeFile(join(parent, 'oo-lite-image'), `sha256:${'a'.repeat(64)}\n`);
      binaries.set(target, join(parent, 'oo'));
    }
    const output = join(dir, 'stage');
    await stage({ output, notices, binaries });
    const packed = JSON.parse(execFileSync('npm', ['pack', '--json', '--ignore-scripts',
      '--pack-destination', dir], { cwd: output, encoding: 'utf8',
      env: { ...process.env, npm_config_cache: join(dir, 'npm-cache') } }))[0];
    const paths = new Set(packed.files.map(file => file.path));
    for (const target of metadata.ooPlatforms) {
      assert.ok(paths.has(`native/${target}/oo`));
      assert.ok(paths.has(`native/${target}/oo-lite-image`));
    }
    assert.ok(!JSON.parse(await readFile(join(output, 'package.json'))).private);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
