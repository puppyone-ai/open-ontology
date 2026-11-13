import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import {
  chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync,
  readdirSync, realpathSync, rmSync, statSync, symlinkSync, writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { nativeArgs, nativeBinary, timeContext, expandLongOptions, installationReport } from '../bin/oo.mjs';

const source = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const digest = data => createHash('sha256').update(data).digest('hex');
const target = `${process.platform}-${process.arch}`;
const integration = (name, fn) => test(name, { skip: process.platform === 'win32', timeout: 15000 }, fn);

test('post-install check requires a verified MySQL discovery helper', t => {
  const f = fixture(t);
  assert.throws(() => installationReport(f.pkg), /MYSQL_DOCTOR_INSTALL/);
  writeFileSync(join(dirname(f.binary), 'oo-lite-image'), `registry.example/oo@sha256:${'a'.repeat(64)}\n`);
  assert.throws(() => installationReport(f.pkg), /oo-mysql-doctor is missing/);
  const helper = join(dirname(f.binary), 'oo-mysql-doctor');
  copyFileSync(f.binary, helper);
  assert.throws(() => installationReport(f.pkg), /NATIVE_MANIFEST_INVALID/);
  const manifest = JSON.parse(readFileSync(f.manifestPath, 'utf8'));
  manifest.binaries[target].mysql_doctor = {
    path: `native/${target}/oo-mysql-doctor`, sha256: digest(readFileSync(helper)),
  };
  writeFileSync(f.manifestPath, JSON.stringify(manifest));
  assert.equal(installationReport(f.pkg).mysql_doctor, 'packaged_verified');
  writeFileSync(helper, 'tampered');
  assert.throws(() => installationReport(f.pkg), /NATIVE_DIGEST_MISMATCH/);
});

test('business dates use the explicit timezone, not UTC or a fabricated business calendar', () => {
  const value = timeContext('Asia/Shanghai', '2026-10-29T19:43:38Z');
  assert.equal(value.today, '2026-10-30');
  assert.deepEqual(value.yesterday, { start: '2026-10-29', end_exclusive: '2026-10-30', basis: 'local_civil_day' });
  assert.equal(value.previous_business_day, null);
  assert.equal(timeContext('America/New_York', '2026-03-09T02:00:00Z').yesterday.start, '2026-03-07');
  assert.equal(timeContext('Asia/Shanghai', '2024-03-01T00:00:00+08:00').yesterday.start, '2024-02-29');
  for (const timezone of [undefined, '', 'unknown', 'Not/AZone', '+08:00']) {
    assert.throws(() => timeContext(timezone));
  }
  for (const instant of ['2026-02-30T00:00:00Z', '2026-10-30', '2026-10-30T24:00:00Z']) {
    assert.throws(() => timeContext('Asia/Shanghai', instant));
  }
});

test('standard equals options preserve SQL and still enforce endpoint credential isolation', () => {
  assert.deepEqual(expandLongOptions(['query', '--timeout-ms=1000', "SELECT '--auth=external'", '--', '--gateway=text']),
    ['query', '--timeout-ms', '1000', "SELECT '--auth=external'", '--', '--gateway=text']);
  const profile = { gateway: 'https://first.example', token_file: '/private/token' };
  assert.throws(() => nativeArgs(['query', '--gateway=https://second.example', 'SELECT 1'], profile),
    /GATEWAY_CREDENTIAL_REQUIRED/);
  const args = nativeArgs(['query', '--gateway=https://first.example', 'SELECT 1'], profile);
  assert.equal(args.filter(arg => arg === '--gateway').length, 1);
  assert(!args.some(arg => arg.startsWith('--gateway=')));
});

integration('time help is offline and configured timezone enriches successful freshness only', t => {
  const f = fixture(t);
  assert.equal(f.run(['time', '--help']).status, 0);
  assert.equal(f.run(['help', 'time']).stdout, f.run(['time', '--help']).stdout);
  assert.notEqual(f.run(['time', '--json']).status, 0);
  const explicit = f.run(['time', '--timezone', 'Asia/Shanghai', '--at', '2026-10-29T19:43:38Z', '--json']);
  assert.equal(explicit.status, 0, explicit.stderr);
  assert.equal(JSON.parse(explicit.stdout).today, '2026-10-30');
  f.configure(['--business-timezone', 'Asia/Shanghai']);
  assert.equal(JSON.parse(f.run(['config', 'show', '--json']).stdout).business_timezone, 'Asia/Shanghai');
  assert.equal(JSON.parse(f.run(['time', '--json']).stdout).timezone, 'Asia/Shanghai');
  const fresh = f.run(['freshness', '--summary', '--json']);
  assert.equal(fresh.status, 0, fresh.stderr);
  const value = JSON.parse(fresh.stdout);
  assert.equal(value.business_time.timezone, 'Asia/Shanghai');
  assert(!value.argv.includes('--business-timezone'));
  const failed = f.run(['freshness', '--json'], { env: { ...f.env, FAKE_EXIT: '2' } });
  assert.equal(failed.status, 2);
  assert(!Object.hasOwn(JSON.parse(failed.stdout), 'business_time'));
});

integration('wrapper errors obey JSON mode, classify bad parameters and never echo credentials', t => {
  const f = fixture(t);
  const badTime = f.run(['time', '--timezone=Not/AZone', '--json']);
  assert.equal(badTime.status, 2);
  assert.equal(badTime.stderr, '');
  const timeError = JSON.parse(badTime.stdout);
  assert.equal(timeError.class, 'usage');
  assert.match(timeError.hint, /oo time --help/);
  f.configure();
  writeFileSync(join(f.dir, 'config.json'), '{"private":"never-echo-this",bad');
  const config = f.run(['freshness', '--summary', '--json']);
  assert.equal(config.status, 1);
  assert.equal(config.stderr, '');
  const error = JSON.parse(config.stdout);
  assert.equal(error.code, 'CONFIG_INVALID');
  assert.equal(error.retry.attempt_count, 0);
  assert(!config.stdout.includes('never-echo-this'));
  assert.match(error.hint, /oo configure --help/);
});

integration('timezone can be set without reentering credentials and never follows a different gateway', t => {
  const f = fixture(t);
  assert.equal(f.run(['config', 'set', '--help']).status, 0);
  const before = f.configure(['--connect-ip', '192.0.2.10']);
  const result = f.run(['config', 'set', '--business-timezone=Asia/Shanghai', '--json']);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).credentials_changed, false);
  assert.deepEqual(JSON.parse(readFileSync(join(f.dir, 'config.json'), 'utf8')),
    { ...before, business_timezone: 'Asia/Shanghai' });
  const redirected = f.run(['freshness', '--json', '--gateway=https://second.example', '--token-stdin'],
    { input: 'separate-fixture-token' });
  assert.equal(redirected.status, 0, redirected.stderr);
  assert(!JSON.parse(redirected.stdout).business_time);
  const bad = f.run(['config', 'set', '--business-timezone=Not/AZone', '--json']);
  assert.equal(bad.status, 2);
  assert.equal(JSON.parse(readFileSync(join(f.dir, 'config.json'), 'utf8')).business_timezone, 'Asia/Shanghai');
});

function fixture(t) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'ontology-cli-test-')));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const pkg = join(root, 'package');
  const dir = join(root, 'private-profile');
  const wrapper = join(pkg, 'bin', 'oo.mjs');
  const binary = join(pkg, 'native', target, process.platform === 'win32' ? 'oo.exe' : 'oo');
  mkdirSync(dirname(wrapper), { recursive: true });
  mkdirSync(dirname(binary), { recursive: true });
  copyFileSync(join(source, 'bin', 'oo.mjs'), wrapper);
  copyFileSync(join(source, 'bin', 'auth.mjs'), join(pkg, 'bin', 'auth.mjs'));
  chmodSync(wrapper, 0o755);
  for (const name of ['package.json', 'README.md']) copyFileSync(join(source, name), join(pkg, name));
  const fake = `#!/usr/bin/env node
import {createHash} from 'node:crypto';
if (process.argv[2] === 'wait') {
  process.on('SIGTERM', () => process.exit(37));
  setInterval(() => {}, 1000);
  process.stdout.write('ready\\n');
} else {
  (async () => {
    const chunks = [];
    for await (const chunk of process.stdin) chunks.push(chunk);
    const stdin = Buffer.concat(chunks);
    process.stdout.write(JSON.stringify({
      argv: process.argv.slice(2), stdin_bytes: stdin.length,
      stdin_sha256: createHash('sha256').update(stdin).digest('hex')
    }));
    process.stderr.write(process.env.FAKE_STDERR || '');
    process.exitCode = Number(process.env.FAKE_EXIT || 0);
  })();
}
`;
  writeFileSync(binary, fake, { mode: 0o755 });
  const manifest = { schema_version: 1, binaries: {
    [target]: { path: `native/${target}/${process.platform === 'win32' ? 'oo.exe' : 'oo'}`, sha256: digest(fake) },
  } };
  const manifestPath = join(pkg, 'native', 'manifest.json');
  writeFileSync(manifestPath, JSON.stringify(manifest));
  const env = { ...process.env, OO_CONFIG_DIR: dir };
  delete env.FAKE_EXIT;
  delete env.FAKE_STDERR;
  function run(args, options = {}) {
    return spawnSync(process.execPath, [wrapper, ...args], {
      cwd: root, env, encoding: 'utf8', input: '', timeout: 10000, ...options,
    });
  }
  function configure(extra = [], token = 'fixture-secret-token') {
    const result = run(['configure', '--gateway', 'https://gateway.example', '--token-stdin', ...extra],
      { input: token });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(readFileSync(join(dir, 'config.json'), 'utf8'));
  }
  return { root, pkg, dir, wrapper, binary, manifest, manifestPath, env, run, configure };
}

function output(result) {
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout);
}

integration('installed package version is readable offline without a native process or profile', t => {
  const f = fixture(t);
  writeFileSync(f.manifestPath, 'invalid manifest');
  f.env.OO_CONFIG_DIR = '/';
  const metadata = JSON.parse(readFileSync(join(source, 'package.json'), 'utf8'));
  assert.deepEqual(output(f.run(['--version', '--json'])), {
    name: metadata.name, version: metadata.version,
  });
  const plain = f.run(['--version']);
  assert.equal(plain.status, 0);
  assert.equal(plain.stdout, `${metadata.name} ${metadata.version}\n`);
  assert.notEqual(f.run(['--version', 'unexpected']).status, 0);
});

integration('all existing commands and arbitrary SQL arguments pass through without a repository', t => {
  const f = fixture(t);
  for (const args of [
    ['query', '--json', 'SELECT COUNT(*) FROM app.orders'],
    ['query', '-f', '-'], ['tables'], ['describe', 'app.orders'],
    ['validate', 'SELECT 1'], ['sql', 'validate', 'SELECT 1'],
    ['freshness', '--summary'], ['knowledge', 'cat', 'README.md'],
    ['knowledge', 'search', '--offset', '20', '--limit', '10', 'order'],
    ['cancel', 'q_actual001'], ['status', 'q_actual001'], ['operations'],
    ['agent-skill', '--json'], ['query', '--help'], ['try', '--role', 'ops'],
    ['quickstart', '--yes'], ['quickstart', '--down', '--json'],
  ]) assert.deepEqual(output(f.run(args)).argv, args);
  const marker = join(f.root, 'shell-must-not-run');
  const sql = `SELECT '--gateway https://evil.example', '$(touch ${marker})';\n-- " \`touch ${marker}\` ; & |`;
  assert.deepEqual(output(f.run(['query', '--json', sql])).argv, ['query', '--json', sql]);
  assert.equal(existsSync(marker), false);
  const stdinSql = 'SELECT 1;\nSELECT 2;';
  assert.equal(output(f.run(['query', '-f', '-'], { input: stdinSql })).stdin_sha256, digest(stdinSql));
});

integration('native stdout, stderr and exit code pass through', t => {
  const f = fixture(t);
  const result = f.run(['tables'], { env: { ...f.env, FAKE_STDERR: 'fixture stderr', FAKE_EXIT: '23' } });
  assert.equal(result.status, 23);
  assert.equal(result.stderr, 'fixture stderr');
  assert.deepEqual(JSON.parse(result.stdout).argv, ['tables']);
});

integration('demo try uses the saved profile while quickstart cleanup does not', t => {
  const f = fixture(t);
  const profile = f.configure();
  const args = output(f.run(['try', '--role', 'ops', '--json'])).argv;
  assert(args.includes(profile.token_file));
  assert.deepEqual(args.slice(-4), ['--transport', 'production', '--auth', 'external']);
  assert.deepEqual(output(f.run(['try', '--help'])).argv, ['try', '--help']);
  assert.deepEqual(output(f.run(['quickstart', '--down', '--json'])).argv,
    ['quickstart', '--down', '--json']);
});

integration('stdin token stays private, off argv, off display and out of the package', t => {
  const f = fixture(t);
  const token = 'fixture-private-secret-123';
  const configured = f.run(['configure', '--gateway', 'https://gateway.example', '--token-stdin'],
    { input: `${token}\r\n` });
  assert.equal(configured.status, 0, configured.stderr);
  assert(!`${configured.stdout}${configured.stderr}`.includes(token));
  const profileText = readFileSync(join(f.dir, 'config.json'), 'utf8');
  const profile = JSON.parse(profileText);
  assert(!profileText.includes(token));
  assert.equal(readFileSync(profile.token_file, 'utf8'), token);
  assert.equal(statSync(f.dir).mode & 0o777, 0o700);
  for (const path of [join(f.dir, 'config.json'), profile.token_file]) {
    assert.equal(statSync(path).mode & 0o777, 0o600);
  }
  const shown = f.run(['config', 'show', '--json']);
  assert.equal(output(shown).token_configured, true);
  assert(!shown.stdout.includes(token));
  assert(!shown.stdout.includes(profile.token_file));
  const args = output(f.run(['query', 'SELECT 1'])).argv;
  assert(!args.includes(token));
  assert(args.includes(profile.token_file));
  assert.deepEqual(args.slice(-4), ['--transport', 'production', '--auth', 'external']);
  assert(!args.includes('--ca-file'));
  assert(!args.includes('--client-pem'));
  assert.deepEqual(readdirSync(join(f.pkg, 'native')).sort(), [target, 'manifest.json'].sort());
  assert(!existsSync(join(f.pkg, 'config.json')));
});

integration('external token and optional TLS references are absolute and never copied or modified', t => {
  const f = fixture(t);
  for (const name of ['token', 'ca.pem', 'client.pem']) writeFileSync(join(f.root, name), `fixture-${name}`, { mode: 0o600 });
  const result = f.run(['configure', '--gateway', 'https://gateway.example/base', '--token-file', 'token',
    '--connect-ip', '192.0.2.10', '--interface', 'en0', '--ca-file', 'ca.pem', '--client-pem', 'client.pem']);
  assert.equal(result.status, 0, result.stderr);
  const profile = JSON.parse(readFileSync(join(f.dir, 'config.json')));
  assert.equal(profile.token_file, join(f.root, 'token'));
  assert.equal(profile.ca_file, join(f.root, 'ca.pem'));
  assert.equal(profile.client_pem, join(f.root, 'client.pem'));
  assert.equal(readFileSync(join(f.root, 'token'), 'utf8'), 'fixture-token');
  assert.deepEqual(readdirSync(f.dir), ['config.json']);
  const argv = output(f.run(['tables'])).argv;
  for (const value of ['192.0.2.10', 'en0', profile.ca_file, profile.client_pem]) assert(argv.includes(value));
  const shown = output(f.run(['config', 'show', '--json']));
  assert.equal(shown.ca_file_configured, true);
  assert.equal(shown.client_pem_configured, true);
  assert(!JSON.stringify(shown).includes('fixture-token'));
});

integration('failed edits preserve the profile and reconfiguration commits complete profiles', t => {
  const f = fixture(t);
  const first = f.configure([], 'fixture-first');
  const before = readFileSync(join(f.dir, 'config.json'), 'utf8');
  for (const token of ['', 'secret with spaces', 'one\ntwo', 'a'.repeat(4097), '\0secret']) {
    const result = f.run(['configure', '--gateway', 'https://next.example', '--token-stdin'], { input: token });
    assert.notEqual(result.status, 0);
    assert.equal(readFileSync(join(f.dir, 'config.json'), 'utf8'), before);
    assert(!result.stderr.includes(token) || token === '');
  }
  const second = f.configure([], 'fixture-second');
  assert.notEqual(first.token_file, second.token_file);
  assert.equal(readFileSync(first.token_file, 'utf8'), 'fixture-first');
  assert.equal(readFileSync(second.token_file, 'utf8'), 'fixture-second');
  assert(readdirSync(f.dir).every(name => !name.endsWith('.tmp')));
  // A failed profile rename must not leave the newly supplied credential behind.
  rmSync(join(f.dir, 'config.json'));
  mkdirSync(join(f.dir, 'config.json'));
  const names = readdirSync(f.dir).sort();
  const failed = f.run(['configure', '--gateway', 'https://next.example', '--token-stdin'], { input: 'fixture-rollback' });
  assert.notEqual(failed.status, 0);
  assert.deepEqual(readdirSync(f.dir).sort(), names);
  assert(!failed.stderr.includes('fixture-rollback'));
});

integration('invalid endpoints, flags and routing are rejected without echoing input', t => {
  const f = fixture(t);
  for (const gateway of [
    'http://gateway.example', 'http://127.0.0.1:8888', 'file:///secret',
    'https://fixture-secret@gateway.example', 'https://gateway.example?key=fixture-secret',
    'https://gateway.example#fixture-secret', 'https://gateway.example/../private',
    'https://gateway.example/%2e%2e', 'https://gateway.example\\private', ' https://gateway.example',
  ]) {
    const result = f.run(['configure', '--gateway', gateway, '--token-stdin'], { input: 'fixture-secret' });
    assert.notEqual(result.status, 0, gateway);
    assert(!result.stderr.includes('fixture-secret'));
    assert(!existsSync(f.dir));
  }
  for (const extra of [
    ['--interface', 'bad name'], ['--connect-ip', '192.0.2.1:443'],
    ['--token', 'fixture-secret'], ['--gateway', 'https://other.example'],
    ['--token-stdin'], ['--ca-file', 'missing.pem'], ['--client-pem', f.root],
  ]) assert.notEqual(f.run(['configure', '--gateway', 'https://gateway.example', '--token-stdin', ...extra],
    { input: 'fixture-secret' }).status, 0);
  assert.notEqual(f.run(['configure', '--gateway', 'https://192.0.2.1', '--token-stdin', '--connect-ip', '192.0.2.2'],
    { input: 'fixture-secret' }).status, 0);
  assert.notEqual(f.run(['configure', '--gateway', 'https://gateway.example']).status, 0);
  assert.notEqual(f.run(['configure', '--gateway', 'https://gateway.example', '--token-file', 'missing', '--token-stdin']).status, 0);
});

integration('offline help and agent-skill ignore invalid profiles and even invalid OO_CONFIG_DIR', t => {
  const f = fixture(t);
  mkdirSync(f.dir, { mode: 0o700 });
  writeFileSync(join(f.dir, 'config.json'), '{"secret":"fixture-secret",broken', { mode: 0o600 });
  for (const args of [['query', '--help'], ['agent-skill'], ['agent-skill', '--json']]) {
    assert.deepEqual(output(f.run(args)).argv, args);
    assert.deepEqual(output(f.run(args, { env: { ...f.env, OO_CONFIG_DIR: '' } })).argv, args);
  }
  const failed = f.run(['tables']);
  assert.notEqual(failed.status, 0);
  assert.match(failed.stderr, /CONFIG_INVALID/);
  assert(!failed.stderr.includes('fixture-secret'));
});

integration('top-level help discovers wrapper commands while native subcommand help is unchanged', t => {
  const f = fixture(t);
  for (const flag of ['--help', '-h']) {
    const result = f.run([flag], { env: { ...f.env, OO_CONFIG_DIR: '' } });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /oo configure --gateway/);
    assert.match(result.stdout, /oo config show --json/);
    assert.match(result.stdout, /oo and ontology are aliases/);
    assert.match(result.stdout, /no Skill, model, database driver or repository required/);
    assert.match(result.stdout, /oo describe app.orders/);
    assert.match(result.stdout, /OO_CONFIG_DIR/);
    assert.deepEqual(JSON.parse(result.stdout.slice(result.stdout.indexOf('{"argv"'))).argv, [flag]);
  }
  assert.deepEqual(output(f.run(['query', '--help'])).argv, ['query', '--help']);
});

integration('help aliases and config help work without profiles or credentials', t => {
  const f = fixture(t);
  f.env.OO_CONFIG_DIR = '';
  assert.equal(f.run(['help']).stdout, f.run(['--help']).stdout);
  assert.equal(f.run(['help', '--help']).stdout, f.run(['--help']).stdout);
  assert.equal(f.run(['help', '-h']).status, 0);
  for (const topic of [['query'], ['knowledge', 'search']]) {
    assert.deepEqual(output(f.run(['help', ...topic])).argv, [...topic, '--help']);
  }
  for (const args of [['configure', '--help'], ['help', 'configure'],
                     ['config', '--help'], ['config', 'show', '--help'],
                     ['help', 'config'], ['help', 'config', 'show']]) {
    const result = f.run(args);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Usage: oo config/);
    assert.match(result.stdout, /OO_CONFIG_DIR/);
    assert(!result.stdout.includes('{"argv"'));
  }
  assert.notEqual(f.run(['help', '--token-file']).status, 0);
  assert.deepEqual(output(f.run(['-V', '--json'])), output(f.run(['--version', '--json'])));
});

integration('MCP help works without a bridge but launching requires a verified bridge', t => {
  const f = fixture(t);
  for (const args of [['mcp', '--help'], ['mcp', '-h'], ['help', 'mcp']]) {
    assert.equal(f.run(args).status, 0);
  }
  assert.notEqual(f.run(['mcp', '--stdio', '--profile', 'example']).status, 0);
});

integration('untrusted saved config, permissive files and symlink config directories fail closed', t => {
  const f = fixture(t);
  const valid = f.configure();
  for (const patch of [
    { schema_version: 2 }, { gateway: 'https://secret@gateway.example' }, { token_file: 'relative' },
    { token: 'fixture-private' }, { connect_ip: 'not-an-ip' }, { interface: 'has space' },
    { ca_file: false }, { client_pem: '/path\ninvalid' },
  ]) {
    writeFileSync(join(f.dir, 'config.json'), JSON.stringify({ ...valid, ...patch }));
    const result = f.run(['tables']);
    assert.notEqual(result.status, 0);
    assert(!result.stderr.includes('fixture-private'));
  }
  writeFileSync(join(f.dir, 'config.json'), JSON.stringify(valid));
  chmodSync(join(f.dir, 'config.json'), 0o644);
  assert.match(f.run(['tables']).stderr, /CONFIG_PERMISSIONS/);
  chmodSync(join(f.dir, 'config.json'), 0o600);
  const link = join(f.root, 'config-link');
  symlinkSync(f.dir, link);
  const result = f.run(['configure', '--gateway', 'https://gateway.example', '--token-stdin'],
    { env: { ...f.env, OO_CONFIG_DIR: link }, input: 'fixture-secret' });
  assert.match(result.stderr, /CONFIG_PERMISSIONS/);
});

integration('explicit flags override defaults once, token stdin passes unchanged and SQL stays after --', t => {
  const f = fixture(t);
  const profile = f.configure(['--connect-ip', '192.0.2.10', '--interface', 'en0']);
  const args = ['query', '--token-stdin', '--transport', 'local-development', '--auth', 'external',
    '--connect-ip', '192.0.2.20', '--interface', 'en1', '--', 'SELECT 1'];
  const result = output(f.run(args, { input: 'fixture-stdin-token' }));
  assert.equal(result.stdin_sha256, digest('fixture-stdin-token'));
  assert(!result.argv.includes('--token-file'));
  assert(!result.argv.includes(profile.token_file));
  for (const flag of ['--transport', '--auth', '--connect-ip', '--interface']) {
    assert.equal(result.argv.filter(value => value === flag).length, 1);
  }
  assert.deepEqual(result.argv.slice(-2), ['--', 'SELECT 1']);
  const sandbox = output(f.run(['tables', '--auth', 'sandbox'])).argv;
  assert(!sandbox.includes('--token-file'));
});

integration('a different gateway requires explicit credentials and never inherits TLS identity or routes', t => {
  const f = fixture(t);
  for (const file of ['ca.pem', 'client.pem']) writeFileSync(join(f.root, file), 'fixture');
  const profile = f.configure(['--ca-file', join(f.root, 'ca.pem'), '--client-pem', join(f.root, 'client.pem'),
    '--connect-ip', '192.0.2.10', '--interface', 'en0']);
  for (const gateway of ['https://other.example', 'https://gateway.example/base', 'https://gateway.example:444']) {
    const denied = f.run(['tables', '--gateway', gateway]);
    assert.notEqual(denied.status, 0);
    assert.match(denied.stderr, /GATEWAY_CREDENTIAL_REQUIRED/);
    assert.equal(denied.stdout, '');
    const argv = output(f.run(['tables', '--gateway', gateway, '--token-stdin'], { input: 'new-secret' })).argv;
    for (const flag of ['--token-file', '--ca-file', '--client-pem', '--connect-ip', '--interface']) {
      assert(!argv.includes(flag), flag);
    }
    assert(!argv.includes(profile.token_file));
  }
  const newCa = join(f.root, 'new-ca.pem');
  writeFileSync(newCa, 'fixture-new-ca');
  const override = output(f.run(['tables', '--gateway', 'https://other.example',
    '--token-file', '/new/token', '--ca-file', newCa, '--client-pem', '/new/client'])).argv;
  for (const value of ['/new/token', newCa, '/new/client']) assert(override.includes(value));
  for (const value of [profile.token_file, profile.ca_file, profile.client_pem]) assert(!override.includes(value));
  const same = output(f.run(['tables', '--gateway', 'https://GATEWAY.EXAMPLE:443/'])).argv;
  for (const value of [profile.token_file, profile.ca_file, profile.client_pem]) assert(same.includes(value));
  for (const gateways of [
    ['--gateway', profile.gateway, '--gateway', 'https://other.example'],
    ['--gateway', 'https://other.example', '--gateway', profile.gateway],
    ['--gateway', profile.gateway, '--gateway=https://other.example'],
  ]) {
    const denied = f.run(['tables', ...gateways]);
    assert.match(denied.stderr, /Repeated --gateway/);
    assert.equal(denied.stdout, '');
  }
});

test('endpoint paths are compared independently from host, without parsing SQL as options', () => {
  const profile = { gateway: 'https://gateway.example/base/', token_file: '/private/token' };
  assert(nativeArgs(['query', '--gateway', 'https://GATEWAY.example:443/base', 'SELECT 1'], profile).includes('/private/token'));
  assert.throws(() => nativeArgs(['query', '--gateway', 'https://gateway.example/other', 'SELECT 1'], profile),
    /GATEWAY_CREDENTIAL_REQUIRED/);
  assert.throws(() => nativeArgs(['tables', '--gateway', 'https://secret@gateway.example/base'], profile), /GATEWAY_INVALID/);
  const sql = "SELECT '--gateway', 'https://other.example', '--token-file'";
  const argv = nativeArgs(['query', sql], profile);
  assert.equal(argv[1], sql);
  assert(argv.includes('/private/token'));
});

test('manifest limits platforms and refuses traversal, missing binaries, symlinks and mismatched digests', t => {
  const f = fixture(t);
  assert.equal(nativeBinary(f.pkg), realpathSync(f.binary));
  assert.throws(() => nativeBinary(f.pkg, 'unsupported', 'cpu'), /UNSUPPORTED_PLATFORM/);
  const entry = f.manifest.binaries[target];
  for (const patch of [{ path: '../../oo' }, { sha256: 'bad' }, { sha256: ['0'.repeat(64)] }]) {
    writeFileSync(f.manifestPath, JSON.stringify({ schema_version: 1, binaries: { [target]: { ...entry, ...patch } } }));
    assert.throws(() => nativeBinary(f.pkg), /NATIVE_MANIFEST_INVALID/);
  }
  writeFileSync(f.manifestPath, JSON.stringify(f.manifest));
  writeFileSync(f.binary, 'tampered');
  assert.throws(() => nativeBinary(f.pkg), /NATIVE_DIGEST_MISMATCH/);
  rmSync(f.binary);
  assert.throws(() => nativeBinary(f.pkg), /NATIVE_BINARY_MISSING/);
  if (process.platform !== 'win32') {
    symlinkSync(process.execPath, f.binary);
    assert.throws(() => nativeBinary(f.pkg), /NATIVE_BINARY_MISSING/);
  }
  rmSync(f.manifestPath);
  assert.throws(() => nativeBinary(f.pkg), /NATIVE_MANIFEST_INVALID/);
});

integration('missing packaged binary never uses a global oo, even for help', t => {
  const f = fixture(t);
  const old = join(f.root, 'old-global');
  mkdirSync(old);
  writeFileSync(join(old, 'oo'), '#!/bin/sh\necho OLD_GLOBAL_FALLBACK\n', { mode: 0o755 });
  rmSync(f.binary);
  const result = f.run(['--help'], { env: { ...f.env, PATH: `${old}:${process.env.PATH}` } });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /NATIVE_BINARY_MISSING/);
  assert(!result.stdout.includes('OLD_GLOBAL_FALLBACK'));
});

integration('npm-style symlink entrypoint runs the packaged launcher', t => {
  const f = fixture(t);
  for (const name of ['oo', 'ontology']) {
    const link = join(f.root, name);
    symlinkSync(f.wrapper, link);
    assert.deepEqual(output(spawnSync(link, ['agent-skill'], {
      env: f.env, input: '', encoding: 'utf8', timeout: 10000,
    })).argv, ['agent-skill']);
  }
});

integration('termination signals reach the native process', async t => {
  const f = fixture(t);
  const child = spawn(process.execPath, [f.wrapper, 'wait'], { env: f.env, stdio: ['ignore', 'pipe', 'pipe'] });
  t.after(() => child.kill('SIGKILL'));
  const timer = setTimeout(() => child.kill('SIGKILL'), 5000);
  t.after(() => clearTimeout(timer));
  await Promise.race([
    once(child.stdout, 'data'),
    once(child, 'exit').then(() => { throw new Error('native child exited before ready'); }),
  ]);
  const exited = once(child, 'exit');
  child.kill('SIGTERM');
  assert.deepEqual(await exited, [37, null]);
});

test('package has no dependencies or network lifecycle and npm packs only release assets', t => {
  const f = fixture(t);
  const pkg = JSON.parse(readFileSync(join(source, 'package.json')));
  assert.equal(pkg.name, '@reduxxx888/ontology-cli');
  assert.equal(pkg.version, '0.2.0');
  assert.equal(pkg.engines.node, '>=22');
  assert.deepEqual(pkg.bin, { oo: 'bin/oo.mjs', ontology: 'bin/oo.mjs' });
  assert.deepEqual(Object.keys(pkg.scripts), ['postinstall', 'test']);
  assert.equal(pkg.scripts.postinstall, 'node bin/oo.mjs install-check --json');
  for (const key of ['dependencies', 'devDependencies', 'optionalDependencies']) assert(!pkg[key]);
  for (const name of ['LICENSE', 'NOTICE', 'SKILL.md', 'THIRD_PARTY_NOTICES.txt', 'dependencies.json']) {
    writeFileSync(join(f.pkg, name), 'fixture release asset');
  }
  for (const relative of ['scripts/private.mjs', 'test/private.test.mjs', '.env', 'config.json',
    'native/private-token', `native/${target}/client.pem`, 'customer-export.json']) {
    mkdirSync(dirname(join(f.pkg, relative)), { recursive: true });
    writeFileSync(join(f.pkg, relative), 'fixture-private-not-for-registry');
  }
  const result = spawnSync(process.platform === 'win32' ? 'npm.cmd' : 'npm',
    ['pack', '--dry-run', '--json', '--ignore-scripts', '--offline', '--cache', join(f.root, 'npm-cache')],
    { cwd: f.pkg, encoding: 'utf8', timeout: 30000 });
  assert.equal(result.status, 0, result.stderr);
  const files = JSON.parse(result.stdout)[0].files.map(file => file.path).sort();
  assert.deepEqual(files, ['LICENSE', 'NOTICE', 'README.md', 'SKILL.md',
    'THIRD_PARTY_NOTICES.txt', 'dependencies.json', 'bin/oo.mjs', 'bin/auth.mjs',
    `native/${target}/${process.platform === 'win32' ? 'oo.exe' : 'oo'}`, 'native/manifest.json', 'package.json'].sort());
});
