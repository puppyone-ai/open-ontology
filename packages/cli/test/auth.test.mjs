import test from 'node:test';
import assert from 'node:assert/strict';
import { randomBytes } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:https';
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync, rmSync, existsSync, statSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { AuthError, checkVersion, expiryWarning, pollDevice, prompt, negotiate, cachePath, exchange } from '../bin/auth.mjs';

const DAY = 86400000;
const wrapper = resolve(import.meta.dirname, '../bin/oo.mjs');
const packageName = '@reduxxx888/ontology-cli';
function temporary(t) {
  const root = mkdtempSync(join(tmpdir(), 'oo-auth-test-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

test('expiry warnings include the exact seven-day boundary and expired renewal guidance', () => {
  assert.equal(expiryWarning({ expires_at_ms: 7 * DAY + 1 }, 0), '');
  assert.match(expiryWarning({ expires_at_ms: 7 * DAY }, 0), /within 7 days/);
  assert.match(expiryWarning({ expires_at_ms: 0 }, 0), /expired.*renew.*login/);
  assert.throws(() => expiryWarning({ expires_at_ms: null }, 0), /valid expiry/);
});

test('minimum version rejects with exit 9 and an exact pinned upgrade command', () => {
  const meta = { protocol_version: '0.2', minimum_cli_version: '0.2.0' };
  checkVersion(meta, '0.2.0', packageName);
  checkVersion(meta, '0.10.0', packageName);
  assert.throws(() => checkVersion(meta, '0.1.99', packageName),
    error => error.exit === 9 && error.message.includes(`npm install -g ${packageName}@0.2.0`));
  assert.throws(() => checkVersion({ ...meta, protocol_version: '9' }, '0.2.0', packageName), error => error.exit === 10);
  assert.throws(() => checkVersion({ ...meta, minimum_cli_version: '0.2.0; echo' }, '0.2.0', packageName));
});

test('meta cache is checked for 24 hours, invalidated by clock rollback and rechecked for older clients', async t => {
  const dir = temporary(t);
  const context = { dir, packageName, packageVersion: '0.2.0',
    privatePath: path => { statSync(path); },
    atomicWrite: (path, data) => writeFileSync(path, data, { mode: 0o600 }) };
  const profile = { gateway: 'https://gateway.example' };
  let calls = 0;
  const io = async (_profile, path) => {
    assert.equal(path, '/v1/meta');
    calls++;
    return { status: 200, value: { minimum_cli_version: '0.2.0', protocol_version: '0.2', token: 'must-not-cache' } };
  };
  await negotiate(context, profile, io, DAY);
  await negotiate(context, profile, io, 2 * DAY - 1);
  assert.equal(calls, 1);
  assert(!readFileSync(cachePath(dir, profile), 'utf8').includes('must-not-cache'));
  await assert.rejects(negotiate({ ...context, packageVersion: '0.1.0' }, profile, io, DAY + 1), error => error.exit === 9);
  await negotiate(context, profile, io, 2 * DAY);
  assert.equal(calls, 2);
  await negotiate(context, profile, io, DAY);
  assert.equal(calls, 3);
});

test('RFC device polling adds five seconds for every slow_down and doubles after network timeout', async () => {
  const waits = [];
  let now = 0;
  const device = { device_code: 'a'.repeat(64), expires_in: 100 };
  const responses = ['authorization_pending', 'slow_down', 'slow_down', 'timeout', null];
  const result = await pollDevice({}, device, {
    now: () => now,
    wait: async ms => { waits.push(ms); now += ms; },
    io: async (_profile, path, options) => {
      assert.equal(path, '/v1/auth/device/token');
      assert.equal(options.body.client_id, 'oo-cli');
      assert.equal(options.form, true);
      const error = responses.shift();
      if (error === 'timeout') throw new AuthError('GATEWAY_TRANSPORT', 'fixture');
      return error ? { status: 400, value: { error } } : { status: 200, value: { access_token: 'fixture-result' } };
    },
  });
  assert.equal(result, 'fixture-result');
  assert.deepEqual(waits, [5000, 5000, 10000, 15000, 30000]);
});

test('device denial stops polling; expiry never sends a late request', async () => {
  let now = 0, calls = 0;
  const timing = { now: () => now, wait: async ms => { now += ms; } };
  await assert.rejects(pollDevice({}, { device_code: 'a'.repeat(64), expires_in: 5 }, {
    ...timing, io: async () => { calls++; },
  }), error => error.code === 'DEVICE_EXPIRED');
  assert.equal(calls, 0);
  await assert.rejects(pollDevice({}, { device_code: 'a'.repeat(64), expires_in: 600 }, {
    ...timing, io: async () => { calls++; return { status: 400, value: { error: 'access_denied' } }; },
  }), error => error.code === 'GATEWAY_REFUSED');
  assert.equal(calls, 1);
});

test('default prompts contain only single-use enrollment material and honest agent-specific MCP setup', () => {
  for (const agent of [undefined, 'claude', 'cursor', 'codex']) {
    const value = prompt({ packageName, packageVersion: '0.2.0', gateway: 'https://gateway.example',
      material: 'OO-0000-0000', raw: false, agent, expires: DAY });
    assert(value.includes(`${packageName}@0.2.0`));
    assert(value.includes('--code-stdin'));
    assert(!value.includes('ooct_'));
    assert(!value.includes('@latest'));
    assert(value.includes('NULL'));
    if (agent) {
      assert(value.includes(agent));
      assert(value.includes('tools/mcp-gateway'));
      assert(value.includes('cannot connect directly'));
      assert(value.includes('OO_MCP_TOKEN_FILE'));
    }
  }
  const raw = prompt({ packageName, packageVersion: '0.2.0', gateway: 'https://gateway.example',
    material: 'fixture-only', raw: true });
  assert(raw.startsWith('RISK:'));
  assert(raw.includes('--token-stdin'));
});

async function fixture(t) {
  const root = temporary(t);
  const key = join(root, 'server.key'), cert = join(root, 'server.crt');
  const created = spawnSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes',
    '-keyout', key, '-out', cert, '-days', '2', '-subj', '/CN=localhost',
    '-addext', 'subjectAltName=DNS:localhost,IP:127.0.0.1'], { stdio: 'ignore' });
  assert.equal(created.status, 0);
  const token = `ooct_${randomBytes(32).toString('base64url')}`;
  const seen = [];
  let redeemed = false;
  const state = { minimum: '0.2.0', readStatus: 200, expiry: Date.now() + DAY, identityExtra: token };
  const server = createServer({ key: readFileSync(key), cert: readFileSync(cert) }, async (req, res) => {
    seen.push(req.url);
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    let body = {};
    try { body = JSON.parse(Buffer.concat(chunks).toString() || '{}'); } catch { /* form is not used here */ }
    res.setHeader('content-type', 'application/json');
    if (req.url === '/v1/meta') return res.end(JSON.stringify({ minimum_cli_version: state.minimum, protocol_version: '0.2' }));
    if (req.url === '/v1/auth/enroll' && body.code === 'OO-0000-0000' && !redeemed) {
      redeemed = true;
      return res.end(JSON.stringify({ token }));
    }
    if (req.headers.authorization !== `Bearer ${token}`) {
      res.statusCode = 401;
      return res.end('{"error":{"code":"UNAUTHENTICATED"}}');
    }
    if (req.url === '/v1/auth/whoami') return res.end(JSON.stringify({ project_id: 'fixture',
      principal_id: 'alice', role: 'reader', expires_at_ms: state.expiry, protocol_version: '0.2', secret: state.identityExtra }));
    if (req.url === '/v1/tables') {
      res.statusCode = state.readStatus;
      return res.end(state.readStatus === 200 ? '{"tables":[]}' : '{"error":{"code":"FORBIDDEN"}}');
    }
    res.statusCode = 404;
    res.end('{}');
  });
  await new Promise(resolveReady => server.listen(0, '127.0.0.1', resolveReady));
  t.after(() => new Promise(resolveClosed => { server.closeAllConnections(); server.close(resolveClosed); }));
  const gateway = `https://localhost:${server.address().port}`;
  const dir = join(root, 'config');
  async function cli(args, input = '', env = {}) {
    const child = spawn(process.execPath, [wrapper, ...args], {
      env: { ...process.env, OO_CONFIG_DIR: dir, ...env }, stdio: ['pipe', 'pipe', 'pipe'],
    });
    let stdout = '', stderr = '';
    child.stdout.on('data', bytes => { stdout += bytes; });
    child.stderr.on('data', bytes => { stderr += bytes; });
    child.stdin.end(input);
    const status = await new Promise(resolveExit => child.on('exit', resolveExit));
    assert(!stdout.includes(token) && !stderr.includes(token), 'CLI output must not reveal credential material');
    return { status, stdout, stderr };
  }
  return { root, dir, cert, gateway, token, cli, state, seen };
}

test('HTTPS enrollment stores a 0600 reference-only profile after identity and read verification', async t => {
  const f = await fixture(t);
  const result = await f.cli(['login', '--gateway', f.gateway, '--ca-file', f.cert, '--code-stdin', '--json',
    '--business-timezone', 'Asia/Shanghai'], 'OO-0000-0000\n');
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stderr, /within 7 days/);
  assert.deepEqual(f.seen, ['/v1/meta', '/v1/auth/enroll', '/v1/auth/whoami', '/v1/tables']);
  const text = readFileSync(join(f.dir, 'config.json'), 'utf8');
  assert(!text.includes(f.token));
  const profile = JSON.parse(text);
  assert.equal(profile.token_file, join(f.dir, 'tokens/default.token'));
  assert.equal(profile.business_timezone, 'Asia/Shanghai');
  assert.equal(statSync(profile.token_file).mode & 0o777, 0o600);
  assert.equal(statSync(join(f.dir, 'config.json')).mode & 0o777, 0o600);
  assert(readFileSync(profile.token_file, 'utf8') === f.token);
  const who = await f.cli(['whoami', '--json']);
  assert.equal(who.status, 0, who.stderr);
  assert.equal(JSON.parse(who.stdout).principal_id, 'alice');
  assert.equal(f.seen.filter(path => path === '/v1/meta').length, 1);
  const again = await f.cli(['login', '--gateway', f.gateway, '--ca-file', f.cert, '--code-stdin'], 'OO-0000-0000');
  assert.equal(again.status, 1);
  assert.match(again.stderr, /PROFILE_EXISTS/);
  f.state.expiry = Date.now() - 1;
  const expired = await f.cli(['whoami']);
  assert.equal(expired.status, 4);
  assert.match(expired.stderr, /renew.*login/);
});

test('TLS cannot be disabled and minimum-version rejection happens before code redemption', async t => {
  const f = await fixture(t);
  const args = ['login', '--gateway', f.gateway, '--code-stdin', '--json'];
  const untrusted = await f.cli(args, 'OO-0000-0000', { NODE_TLS_REJECT_UNAUTHORIZED: '0' });
  assert.equal(untrusted.status, 6);
  assert.equal(f.seen.length, 0);
  f.state.minimum = '0.3.0';
  const old = await f.cli([...args, '--ca-file', f.cert], 'OO-0000-0000');
  assert.equal(old.status, 9);
  assert.match(old.stdout, /npm install -g @reduxxx888\/ontology-cli@0.3.0/);
  assert.deepEqual(f.seen, ['/v1/meta']);
  assert(!existsSync(join(f.dir, 'tokens/default.token')));
});

test('failed read verification and a symlink token directory never replace an active profile', async t => {
  const f = await fixture(t);
  mkdirSync(f.dir, { mode: 0o700 });
  const old = '{"keep":"original profile"}';
  writeFileSync(join(f.dir, 'config.json'), old, { mode: 0o600 });
  f.state.readStatus = 403;
  const result = await f.cli(['login', '--gateway', f.gateway, '--ca-file', f.cert, '--code-stdin'], 'OO-0000-0000');
  assert.equal(result.status, 4);
  assert.equal(readFileSync(join(f.dir, 'config.json'), 'utf8'), old);
  assert(!existsSync(join(f.dir, 'tokens/default.token')));
  const other = await fixture(t);
  mkdirSync(other.dir, { mode: 0o700 });
  const outside = join(other.root, 'other');
  mkdirSync(outside, { mode: 0o700 });
  symlinkSync(outside, join(other.dir, 'tokens'));
  const denied = await other.cli(['login', '--gateway', other.gateway, '--ca-file', other.cert, '--code-stdin'], 'OO-0000-0000');
  assert.notEqual(denied.status, 0);
  assert(!existsSync(join(outside, 'default.token')));
});

test('HTTPS redirects are never followed with enrollment or bearer credentials', async t => {
  const f = await fixture(t);
  const redirect = createServer({ key: readFileSync(join(f.root, 'server.key')), cert: readFileSync(f.cert) },
    (_req, res) => { res.writeHead(307, { location: `${f.gateway}/v1/auth/enroll`, 'content-type': 'application/json' }); res.end('{}'); });
  await new Promise(resolveReady => redirect.listen(0, '127.0.0.1', resolveReady));
  t.after(() => new Promise(resolveClosed => { redirect.closeAllConnections(); redirect.close(resolveClosed); }));
  const reply = await exchange({ gateway: `https://localhost:${redirect.address().port}`, ca_file: f.cert },
    '/v1/auth/enroll', { body: { code: 'OO-0000-0000' } });
  assert.equal(reply.status, 307);
  assert.equal(f.seen.length, 0);
});

test('explicit connect-ip keeps TLS hostname validation and bypasses only DNS', async t => {
  const f = await fixture(t);
  const reply = await exchange({ gateway: f.gateway, ca_file: f.cert, connect_ip: '127.0.0.1' }, '/v1/meta');
  assert.equal(reply.status, 200);
  await assert.rejects(exchange({
    gateway: f.gateway.replace('localhost', 'wrong-name.invalid'), ca_file: f.cert, connect_ip: '127.0.0.1',
  }, '/v1/meta'), error => error.code === 'GATEWAY_TRANSPORT');
  assert.equal(f.seen.length, 1);
});
