import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import http from 'node:http';
import https from 'node:https';
import net from 'node:net';
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { once } from 'node:events';
import { pathToFileURL } from 'node:url';
import { allowedRoute, createBridge, loadConfig, main, originOptions, MAX_BODY } from './bridge.mjs';

let dir, tls, origin, bridge, port;
const received = [];
async function listen(server) {
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  return server.address().port;
}
async function close(server) {
  if (!server) return;
  const done = new Promise((resolve) => server.close(resolve));
  server.closeAllConnections();
  await done;
}
function send(path, { headers = {}, method = 'GET', body, targetPort = port } = {}) {
  return new Promise((resolve, reject) => {
    const req = http.request({ hostname: '127.0.0.1', port: targetPort, path, method, headers }, (res) => {
      const chunks = [];
      res.on('data', (chunk) => chunks.push(chunk));
      res.on('error', reject);
      res.on('end', () => resolve({
        status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks).toString(),
      }));
    });
    req.on('error', reject);
    req.end(body);
  });
}
before(async () => {
  dir = mkdtempSync(join(tmpdir(), 'gateway-bridge-test-'));
  const openssl = (...args) => execFileSync('openssl', args, { cwd: dir, stdio: 'ignore' });
  openssl('req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
    '-subj', '/CN=bridge-test-ca', '-keyout', 'ca.key', '-out', 'ca.pem');
  for (const name of ['server', 'client']) {
    openssl('req', '-newkey', 'rsa:2048', '-nodes', '-subj', `/CN=${name}`,
      '-keyout', `${name}.key`, '-out', `${name}.csr`);
    writeFileSync(join(dir, `${name}.ext`), name === 'server'
      ? 'subjectAltName=IP:127.0.0.1\nextendedKeyUsage=serverAuth\n'
      : 'extendedKeyUsage=clientAuth\n');
    openssl('x509', '-req', '-in', `${name}.csr`, '-CA', 'ca.pem', '-CAkey', 'ca.key',
      '-CAcreateserial', '-days', '1', '-extfile', `${name}.ext`, '-out', `${name}.pem`);
  }
  const read = (name) => readFileSync(join(dir, name));
  tls = { ca: read('ca.pem'), clientPem: Buffer.concat([read('client.pem'), read('client.key')]) };
  writeFileSync(join(dir, 'client-combined.pem'), tls.clientPem);
  origin = https.createServer({
    ca: tls.ca, cert: read('server.pem'), key: read('server.key'),
    requestCert: true, rejectUnauthorized: true,
  }, (req, res) => {
    const chunks = [];
    req.on('data', (chunk) => chunks.push(chunk));
    req.on('end', () => {
      received.push({
        headers: req.headers, path: req.url, body: Buffer.concat(chunks).toString(),
        authorized: req.socket.authorized,
      });
      if (req.url.includes('stall')) return;
      const requested = req.headers['x-oo-expected-publication-id'];
      if (requested && requested !== 'current' && requested !== `pub1_${'1'.repeat(64)}`) {
        res.writeHead(409, { 'content-type': 'application/json' });
        return res.end(JSON.stringify({ error: {
          code: 'CONFLICT', hint: 'reason=PUBLICATION_DRIFT;action=RECOMPUTE_WHOLE_QUESTION',
          retryable: false,
        } }));
      }
      res.writeHead(req.headers.authorization === 'Bearer unknown' ? 401 : 200, {
        'content-type': 'application/json',
        authorization: 'Bearer must-not-escape', 'set-cookie': 'secret=hidden',
        'x-internal-secret': 'hidden', location: 'https://private.invalid',
        ...(requested && req.headers.authorization !== 'Bearer unknown' ? {
          'x-oo-publication-id': `pub1_${'1'.repeat(64)}`,
          'x-oo-data-vector-sha256': '2'.repeat(64),
        } : {}),
      });
      res.end('{"gateway":true}');
    });
  });
  await listen(origin);
  bridge = createBridge(tls, (options, callback) =>
    https.request({ ...options, port: origin.address().port }, callback));
  port = await listen(bridge);
});
after(async () => {
  await close(bridge);
  await close(origin);
  if (dir) rmSync(dir, { recursive: true, force: true });
});

test('missing, malformed and duplicate Bearer fail locally; health contains no data', async () => {
  const before = received.length;
  for (const headers of [{}, { authorization: 'Basic secret' },
    { authorization: ['Bearer first', 'Bearer second'] }]) {
    const response = await send('/v1/tables', { headers });
    assert.equal(response.status, 401);
    assert.equal(response.body, '{"error":{"code":"BEARER_REQUIRED"}}');
  }
  assert.equal((await send('/health')).body, '{"ok":true}');
  assert.equal(received.length, before);
});

test('only known data/metadata routes; no absolute URL, admin or traversal', async () => {
  for (const path of ['/internal', '/ready', '/v1/admin', '/v1/tables/../admin',
    '/v1/knowledge/%2e%2e/secret', '/v1/knowledge/%252e%252e/secret',
    '/v1/tables%2fsecret', '//evil.invalid/v1/tables', 'https://evil.invalid/v1/tables']) {
    assert.equal(allowedRoute('GET', path), false, path);
    assert.equal((await send(path, { headers: { authorization: 'Bearer user' } })).status, 404);
  }
  assert.equal(allowedRoute('DELETE', '/v1/query'), false);
  assert.equal(allowedRoute('GET', '/v1/knowledge/tables/schema.table.md'), true);
});

test('real mTLS forwards exact per-user Authorization, body, query; strips sensitive headers', async () => {
  for (const token of ['Bearer user-one', 'bEaReR user-two']) {
    const response = await send('/v1/query?format=json', {
      method: 'POST', body: '{"sql":"SELECT 1"}',
      headers: {
        authorization: token, 'content-type': 'application/json', cookie: 'secret=browser',
        'proxy-authorization': 'Basic hidden', 'x-oo-role': 'admin',
        'x-forwarded-client-cert': 'forged', 'x-forwarded-host': 'evil.invalid',
      },
    });
    assert.equal(response.status, 200);
    const actual = received.at(-1);
    assert.equal(actual.authorized, true);
    assert.equal(actual.headers.authorization, token);
    assert.equal(actual.path, '/v1/query?format=json');
    assert.equal(actual.body, '{"sql":"SELECT 1"}');
    assert.equal(actual.headers.host, `127.0.0.1:${origin.address().port}`);
    for (const name of ['cookie', 'proxy-authorization', 'x-oo-role',
      'x-forwarded-client-cert', 'x-forwarded-host']) assert.equal(actual.headers[name], undefined);
    for (const name of ['authorization', 'set-cookie', 'x-internal-secret', 'location']) {
      assert.equal(response.headers[name], undefined);
    }
    assert.equal(response.headers['cache-control'], 'no-store');
  }
  const unknown = await send('/v1/tables', { headers: { authorization: 'Bearer unknown' } });
  assert.equal(unknown.status, 401);
  assert.equal(unknown.body, '{"gateway":true}');
  assert.equal(received.at(-1).headers.authorization, 'Bearer unknown');
});

test('TLS verification is explicit and an untrusted origin fails closed without details', async () => {
  const options = originOptions(tls, { method: 'GET', url: '/v1/tables',
    headers: { authorization: 'Bearer private-token' } }, Buffer.alloc(0));
  assert.equal(options.rejectUnauthorized, true);
  assert.equal(options.hostname, '127.0.0.1');
  assert.equal(options.port, 19481);
  assert.equal(options.agent, false);
  const bad = createBridge({ ...tls, ca: readFileSync(join(dir, 'client.pem')) },
    (opts, callback) => https.request({ ...opts, port: origin.address().port }, callback));
  try {
    const targetPort = await listen(bad);
    const response = await send('/v1/tables', {
      targetPort, headers: { authorization: 'Bearer private-token' },
    });
    assert.equal(response.status, 500);
    assert.deepEqual(JSON.parse(response.body), { error: {
      code: 'INTERNAL', message: 'Gateway origin unavailable',
      hint: 'ORIGIN_UNAVAILABLE', retryable: true,
    } });
    assert.doesNotMatch(JSON.stringify(response), /private-token|CERT|BEGIN|127\.0\.0\.1/);
  } finally {
    await close(bad);
  }
});

test('mTLS preserves both F22 directions without forwarding caller identity or forged actual proofs', async () => {
  const expected = `pub1_${'1'.repeat(64)}`;
  const vector = '2'.repeat(64);
  for (const requested of ['current', expected]) {
    const response = await send('/v1/freshness', { headers: {
      authorization: 'Bearer user', 'x-oo-expected-publication-id': requested,
      ...(requested === 'current' ? {} : { 'x-oo-expected-data-vector-sha256': vector }),
      'x-oo-publication-id': 'forged', 'x-oo-data-vector-sha256': 'forged',
      'x-oo-role': 'admin', cookie: 'private',
    } });
    const incoming = received.at(-1).headers;
    assert.equal(incoming['x-oo-expected-publication-id'], requested);
    assert.equal(incoming['x-oo-expected-data-vector-sha256'], requested === 'current' ? undefined : vector);
    for (const name of ['x-oo-publication-id', 'x-oo-data-vector-sha256', 'x-oo-role', 'cookie']) {
      assert.equal(incoming[name], undefined);
    }
    assert.equal(response.status, 200);
    assert.equal(response.headers['x-oo-publication-id'], expected);
    assert.equal(response.headers['x-oo-data-vector-sha256'], vector);
    assert.equal(response.headers.authorization, undefined);
  }
  const stale = await send('/v1/freshness', { headers: {
    authorization: 'Bearer user', 'x-oo-expected-publication-id': `pub1_${'0'.repeat(64)}`,
    'x-oo-expected-data-vector-sha256': vector,
  } });
  assert.equal(stale.status, 409);
  assert.match(JSON.parse(stale.body).error.hint, /PUBLICATION_DRIFT/);
  assert.equal(stale.headers['x-oo-publication-id'], undefined);
  const legacy = await send('/v1/freshness', { headers: { authorization: 'Bearer user' } });
  assert.equal(legacy.status, 200);
  assert.equal(legacy.headers['x-oo-publication-id'], undefined);
});

test('default deadline is 120 seconds and remains bounded', () => {
  assert.equal(bridge.requestTimeout, 120000);
  assert.equal(bridge.headersTimeout, 120000);
  assert.equal(bridge.timeout, 120000);
  for (const timeout of [0, -1, 120001, Infinity, 1.5]) {
    assert.throws(() => createBridge(tls, undefined, timeout), /CONFIG_INVALID/);
  }
});

test('body boundary, chunked overflow, content encoding and bounded upstream timeout', async () => {
  const headers = { authorization: 'Bearer user' };
  assert.equal((await send('/v1/query', {
    method: 'POST', headers, body: 'x'.repeat(MAX_BODY),
  })).status, 200);
  for (const extra of [{ 'content-length': MAX_BODY + 1 }, { 'transfer-encoding': 'chunked' }]) {
    assert.equal((await send('/v1/query', {
      method: 'POST', headers: { ...headers, ...extra }, body: 'x'.repeat(MAX_BODY + 1),
    })).status, 413);
  }
  assert.equal((await send('/v1/query', {
    method: 'POST', headers: { ...headers, 'content-encoding': 'gzip' }, body: 'x',
  })).status, 415);
  const slow = createBridge(tls,
    (opts, callback) => https.request({ ...opts, port: origin.address().port }, callback), 150);
  try {
    const targetPort = await listen(slow);
    const response = await send('/v1/tables?stall=1', { headers, targetPort });
    assert.equal(response.status, 408);
    assert.deepEqual(JSON.parse(response.body), { error: {
      code: 'TIMEOUT', message: 'Public Gateway request deadline exceeded',
      hint: 'REQUEST_TIMEOUT', retryable: false,
    } });
  } finally {
    await close(slow);
  }
});

test('TLS rejects wrong server identity and missing client certificate', async () => {
  for (const override of [{ servername: 'wrong.example' }, { cert: undefined, key: undefined }]) {
    const isolated = createBridge(tls, (opts, callback) =>
      https.request({ ...opts, ...override, port: origin.address().port }, callback));
    try {
      const before = received.length;
      const targetPort = await listen(isolated);
      const response = await send('/v1/tables', {
        targetPort, headers: { authorization: 'Bearer private-token' },
      });
      assert.equal(response.status, 500);
      assert.deepEqual(JSON.parse(response.body), { error: {
        code: 'INTERNAL', message: 'Gateway origin unavailable',
        hint: 'ORIGIN_UNAVAILABLE', retryable: true,
      } });
      assert.equal(received.length, before);
    } finally {
      await close(isolated);
    }
  }
});

test('Expect: 100-continue works only after local authorization', async () => {
  const response = await new Promise((resolve, reject) => {
    const req = http.request({
      hostname: '127.0.0.1', port, path: '/v1/query', method: 'POST',
      headers: { authorization: 'Bearer user', expect: '100-continue', 'content-length': 2 },
    }, (res) => {
      res.resume();
      res.on('end', () => resolve(res.statusCode));
    });
    req.on('continue', () => req.end('{}'));
    req.on('error', reject);
    req.flushHeaders();
  });
  assert.equal(response, 200);
});

test('CONNECT and Upgrade are rejected without proxying', async () => {
  const before = received.length;
  for (const raw of [
    'CONNECT evil.invalid:443 HTTP/1.1\r\nHost: evil.invalid\r\n\r\n',
    'GET /v1/tables HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n',
  ]) {
    const response = await new Promise((resolve, reject) => {
      const socket = net.connect(port, '127.0.0.1', () => socket.write(raw));
      let text = '';
      socket.on('data', (chunk) => { text += chunk; });
      socket.on('end', () => resolve(text));
      socket.on('error', reject);
    });
    assert.match(response, /^HTTP\/1\.1 400 /);
  }
  assert.equal(received.length, before);
});

test('external configuration pins listener and origin; no shared token/TLS bypass fields', () => {
  const path = join(dir, 'config.json');
  const config = {
    listen: '127.0.0.1:19482', origin: 'https://127.0.0.1:19481',
    originCaFile: join(dir, 'ca.pem'), originClientPemFile: join(dir, 'client-combined.pem'),
  };
  for (const listen of ['127.0.0.1:19482', '127.0.0.1:8080']) {
    writeFileSync(path, JSON.stringify({ ...config, listen }));
    assert.deepEqual(loadConfig(path), { ...tls, listen });
  }
  for (const change of [{ origin: 'https://127.0.0.1:19480' },
    { origin: 'https://example.com:19481' },
    { listen: '0.0.0.0:19482' }, { listen: '0.0.0.0:8080' },
    { listen: '127.0.0.1:8081' }, { listen: 'localhost:8080' },
    { listen: '[::]:8080' }, { token: 'admin' }, { rejectUnauthorized: false }]) {
    writeFileSync(path, JSON.stringify({ ...config, ...change }));
    assert.throws(() => loadConfig(path), /CONFIG_INVALID/);
  }
});

test('main binds and reports the configured loopback port without starting a service', (t) => {
  const bindings = [];
  const output = [];
  t.mock.method(http.Server.prototype, 'listen', function (port, host, ready) {
    bindings.push({ port, host });
    ready();
    return this;
  });
  t.mock.method(process.stdout, 'write', (text) => { output.push(text); return true; });
  const path = join(dir, 'main-config.json');
  for (const port of [19482, 8080]) {
    writeFileSync(path, JSON.stringify({
      listen: `127.0.0.1:${port}`, origin: 'https://127.0.0.1:19481',
      originCaFile: join(dir, 'ca.pem'), originClientPemFile: join(dir, 'client-combined.pem'),
    }));
    assert.ok(main(path) instanceof http.Server);
    assert.deepEqual(bindings.at(-1), { port, host: '127.0.0.1' });
    assert.equal(output.at(-1), `http://127.0.0.1:${port}\n`);
  }
});

test('copied deployment accepts non-repo paths but rejects real repo ancestors', async () => {
  const deployed = join(dir, '.local', 'share', 'ontology-public-gateway');
  mkdirSync(deployed, { recursive: true });
  const entry = join(deployed, 'bridge.mjs');
  copyFileSync(new URL('./bridge.mjs', import.meta.url), entry);
  const copied = await import(pathToFileURL(entry).href);
  const path = join(deployed, 'config.json');
  const config = {
    listen: '127.0.0.1:19482', origin: 'https://127.0.0.1:19481',
    originCaFile: join(dir, 'ca.pem'), originClientPemFile: join(dir, 'client-combined.pem'),
  };
  writeFileSync(path, JSON.stringify(config));
  assert.deepEqual(copied.loadConfig(path), { ...tls, listen: config.listen });
  assert.throws(() => copied.loadConfig('config.json'), /CONFIG_INVALID/);
  for (const marker of ['directory', 'worktree-file']) {
    const repo = join(dir, marker);
    const nested = join(repo, 'nested');
    mkdirSync(nested, { recursive: true });
    if (marker === 'directory') mkdirSync(join(repo, '.git'));
    else writeFileSync(join(repo, '.git'), 'gitdir: /unused/worktree\n');
    const repoConfig = join(nested, 'config.json');
    writeFileSync(repoConfig, JSON.stringify(config));
    assert.throws(() => copied.loadConfig(repoConfig), /CONFIG_INVALID/);
    for (const field of ['originCaFile', 'originClientPemFile']) {
      const pem = join(nested, `${field}.pem`);
      copyFileSync(config[field], pem);
      const link = join(deployed, `${marker}-${field}.pem`);
      symlinkSync(pem, link);
      writeFileSync(path, JSON.stringify({ ...config, [field]: link }));
      assert.throws(() => copied.loadConfig(path), /CONFIG_INVALID/);
    }
  }
});

test('startup errors print only a safe code, never paths or exception details', () => {
  const entry = new URL('./bridge.mjs', import.meta.url);
  const result = spawnSync(process.execPath, [entry.pathname, '/private-token-SQL/missing.json'], {
    encoding: 'utf8',
  });
  assert.equal(result.status, 1);
  assert.equal(result.stdout, '');
  assert.equal(result.stderr, 'BRIDGE_CONFIG_INVALID\n');
});
