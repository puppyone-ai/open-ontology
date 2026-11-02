import http from 'node:http';
import https from 'node:https';
import { lstatSync, readFileSync, realpathSync } from 'node:fs';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createSecureContext } from 'node:tls';

export const ORIGIN = 'https://127.0.0.1:19481';
export const LISTEN = '127.0.0.1:19482';
export const MAX_BODY = 65536;
const TIMEOUT_MS = 120000;
const EXPECTED_BINDING_HEADERS = [
  'x-oo-expected-publication-id', 'x-oo-expected-data-vector-sha256',
];
const ACTUAL_BINDING_HEADERS = ['x-oo-publication-id', 'x-oo-data-vector-sha256'];

export function loadConfig(filename) {
  const outsideRepo = (path) => {
    if (typeof path !== 'string' || !isAbsolute(path)) throw new Error('CONFIG_INVALID');
    const actual = realpathSync(path);
    for (let parent = dirname(actual); ; parent = dirname(parent)) {
      if (lstatSync(join(parent, '.git'), { throwIfNoEntry: false })) {
        throw new Error('CONFIG_INVALID');
      }
      if (parent === dirname(parent)) break;
    }
    return actual;
  };
  const config = JSON.parse(readFileSync(outsideRepo(filename), 'utf8'));
  const keys = ['listen', 'origin', 'originCaFile', 'originClientPemFile'];
  if (!config || Object.keys(config).some((key) => !keys.includes(key))
      || ![LISTEN, '127.0.0.1:8080'].includes(config.listen) || config.origin !== ORIGIN) {
    throw new Error('CONFIG_INVALID');
  }
  const ca = readFileSync(outsideRepo(config.originCaFile));
  const clientPem = readFileSync(outsideRepo(config.originClientPemFile));
  createSecureContext({ ca, cert: clientPem, key: clientPem, minVersion: 'TLSv1.2' });
  return { ca, clientPem, listen: config.listen };
}

export function allowedRoute(method, target) {
  // Match raw origin-form paths: never normalize an absolute URL or traversal.
  if (!target?.startsWith('/') || target.startsWith('//')
      || /[\\#\x00-\x20\x7f]/.test(target)) return false;
  let path;
  try {
    const raw = target.split('?')[0];
    if (/%(?:2f|5c|25)/i.test(raw)) return false;
    path = decodeURIComponent(raw);
  } catch {
    return false;
  }
  if (/[\\#?\x00-\x20\x7f]/.test(path)
      || path.split('/').some((part) => part === '.' || part === '..')) return false;
  if (method === 'GET' || method === 'HEAD') {
    return /^\/v1\/(?:session\/current|pack\/metadata|tables(?:\/[^/]+)?|freshness|knowledge\/[^/]+(?:\/[^/]+)*|queries\/[^/]+|operations(?:\/[^/]+)?)$/.test(path);
  }
  return method === 'POST'
    && /^\/v1\/(?:sql(?:\/validate)?|query|knowledge\/search|queries\/[^/]+\/cancel|operations\/[^/]+\/cancel)$/.test(path);
}

export function originOptions(tls, request, body) {
  return {
    protocol: 'https:', hostname: '127.0.0.1', port: 19481,
    method: request.method, path: request.url,
    ca: tls.ca, cert: tls.clientPem, key: tls.clientPem,
    rejectUnauthorized: true, minVersion: 'TLSv1.2', agent: false,
    headers: {
      authorization: request.headers.authorization,
      ...(request.headers['content-type']
        ? { 'content-type': request.headers['content-type'] } : {}),
      ...Object.fromEntries(EXPECTED_BINDING_HEADERS
        .filter((name) => request.headers[name] !== undefined)
        .map((name) => [name, request.headers[name]])),
      accept: 'application/json',
      'content-length': body.length,
    },
  };
}

function fail(response, status, code) {
  if (response.destroyed || response.writableEnded) return;
  if (response.headersSent) return response.destroy();
  // Data clients understand the Gateway error contract, not proxy-specific codes.
  const error = status === 502 ? {
    code: 'INTERNAL', message: 'Gateway origin unavailable', hint: code, retryable: true,
  } : status === 504 ? {
    code: 'TIMEOUT', message: 'Public Gateway request deadline exceeded', hint: code, retryable: false,
  } : { code };
  const wireStatus = status === 502 ? 500 : status === 504 ? 408 : status;
  response.writeHead(wireStatus, {
    'content-type': 'application/json',
    'cache-control': 'no-store',
    connection: 'close',
    ...(status === 401 ? { 'www-authenticate': 'Bearer' } : {}),
  });
  response.end(JSON.stringify({ error }));
}

// The transport argument exists only for isolated tests; config cannot replace it.
export function createBridge(tls, transport = https.request, timeoutMs = TIMEOUT_MS) {
  if (!Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > TIMEOUT_MS) {
    throw new Error('CONFIG_INVALID');
  }
  const handle = (request, response) => {
    if (request.headers.upgrade || /\bupgrade\b/i.test(request.headers.connection ?? '')) {
      return fail(response, 400, 'UPGRADE_REJECTED');
    }
    if ((request.method === 'GET' || request.method === 'HEAD') && request.url === '/health') {
      response.writeHead(200, {
        'content-type': 'application/json', 'cache-control': 'no-store', connection: 'close',
      });
      return response.end('{"ok":true}');
    }
    const authCount = request.rawHeaders.filter((_, index) =>
      index % 2 === 0 && request.rawHeaders[index].toLowerCase() === 'authorization').length;
    if (authCount !== 1 || !/^Bearer [A-Za-z0-9._~+/-]+=*$/i.test(request.headers.authorization ?? '')) {
      return fail(response, 401, 'BEARER_REQUIRED');
    }
    if (!allowedRoute(request.method, request.url)) return fail(response, 404, 'ROUTE_REJECTED');
    if (request.headers['content-encoding']
        && request.headers['content-encoding'].toLowerCase() !== 'identity') {
      return fail(response, 415, 'ENCODING_REJECTED');
    }
    if (Number(request.headers['content-length']) > MAX_BODY) {
      return fail(response, 413, 'BODY_TOO_LARGE');
    }
    let upstream;
    let incoming;
    let stopped = false;
    const stop = (status, code) => {
      if (stopped) return;
      stopped = true;
      fail(response, status, code);
      upstream?.destroy();
      incoming?.destroy();
    };
    const deadline = setTimeout(() => stop(504, 'REQUEST_TIMEOUT'), timeoutMs);
    deadline.unref();
    response.on('close', () => {
      clearTimeout(deadline);
      stopped = true;
      upstream?.destroy();
      incoming?.destroy();
    });
    request.on('error', () => stop(400, 'REQUEST_INVALID'));
    request.on('aborted', () => stop(400, 'REQUEST_ABORTED'));
    const chunks = [];
    let length = 0;
    request.on('data', (chunk) => {
      if (stopped) return;
      length += chunk.length;
      if (length > MAX_BODY) return stop(413, 'BODY_TOO_LARGE');
      chunks.push(chunk);
    });
    request.on('end', () => {
      if (stopped) return;
      const body = Buffer.concat(chunks, length);
      try {
        upstream = transport(originOptions(tls, request, body), (result) => {
          incoming = result;
          if (stopped) return result.destroy();
          // Preserve publication consistency proofs, never identity/auth headers.
          const headers = { 'cache-control': 'no-store', connection: 'close' };
          for (const name of ['content-type', 'content-length', 'content-encoding', ...ACTUAL_BINDING_HEADERS]) {
            if (result.headers[name] !== undefined) headers[name] = result.headers[name];
          }
          result.on('error', () => stop(502, 'ORIGIN_RESPONSE_FAILED'));
          result.on('aborted', () => stop(502, 'ORIGIN_RESPONSE_FAILED'));
          response.writeHead(result.statusCode, headers);
          result.pipe(response);
        });
        upstream.on('error', () => stop(502, 'ORIGIN_UNAVAILABLE'));
        upstream.on('upgrade', (_message, socket) => {
          socket.destroy();
          stop(502, 'ORIGIN_UPGRADE_REJECTED');
        });
        upstream.end(body);
      } catch {
        stop(502, 'ORIGIN_UNAVAILABLE');
      }
    });
    if (request.headers.expect?.toLowerCase() === '100-continue') response.writeContinue();
  };
  const server = http.createServer({
    maxHeaderSize: 16384, headersTimeout: timeoutMs, requestTimeout: timeoutMs,
    connectionsCheckingInterval: Math.min(timeoutMs, 1000),
  }, handle);
  server.on('checkContinue', handle);
  server.on('checkExpectation', (_request, response) => fail(response, 417, 'EXPECTATION_REJECTED'));
  const rejectSocket = (socket) => {
    socket.on('error', () => {});
    socket.end('HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n');
  };
  server.on('connect', (_request, socket) => rejectSocket(socket));
  server.on('upgrade', (_request, socket) => rejectSocket(socket));
  server.on('clientError', (_error, socket) => rejectSocket(socket));
  server.setTimeout(timeoutMs, (socket) => socket.destroy());
  return server;
}

export function main(filename) {
  try {
    if (process.env.NODE_TLS_REJECT_UNAUTHORIZED === '0'
        || process.env.NODE_DEBUG || process.env.NODE_DEBUG_NATIVE) throw new Error('CONFIG_INVALID');
    const config = loadConfig(filename);
    const server = createBridge(config);
    server.on('error', () => {
      process.stderr.write('BRIDGE_LISTEN_FAILED\n');
      process.exitCode = 1;
    });
    server.listen(Number(config.listen.split(':')[1]), '127.0.0.1', () => {
      process.stdout.write(`http://${config.listen}\n`);
    });
    return server;
  } catch {
    process.stderr.write('BRIDGE_CONFIG_INVALID\n');
    process.exitCode = 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.length === 3 ? process.argv[2] : undefined);
}
