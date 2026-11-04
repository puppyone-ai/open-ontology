import { createHash } from 'node:crypto';
import { request } from 'node:https';
import { readFileSync, statSync, existsSync, mkdirSync } from 'node:fs';
import { isIP } from 'node:net';
import { networkInterfaces } from 'node:os';
import { join, resolve } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';

export class AuthError extends Error {
  constructor(code, message, exit = 1) {
    super(message);
    this.code = code;
    this.exit = exit;
  }
}
const refuse = (code, message, exit) => { throw new AuthError(code, message, exit); };
const DAY = 86400000;
const CLIENT = 'oo-cli';
const GRANT = 'urn:ietf:params:oauth:grant-type:device_code';
const codePattern = /^OO-[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$/;
const tokenPattern = /^ooct_[A-Za-z0-9_-]{43}$/;
const shell = text => `'${text.replaceAll("'", "'\\''")}'`;

function readBounded(path, max) {
  const stat = statSync(path);
  if (!stat.isFile() || stat.size > max) refuse('AUTH_FILE_INVALID', 'Authentication file is not a bounded regular file.');
  const bytes = readFileSync(path);
  if (bytes.length > max) refuse('AUTH_FILE_INVALID', 'Authentication file exceeds its byte limit.');
  return bytes;
}

function endpoint(gateway, path) {
  const url = new URL(gateway);
  const prefix = url.pathname.replace(/\/$/, '');
  return new URL(`${prefix}${path}`, url.origin);
}

export async function exchange(profile, path, { body, token, form = false, admin = false } = {}) {
  const url = endpoint(profile.gateway, path);
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash) {
    refuse('GATEWAY_INVALID', 'TLS-verified HTTPS is required.', 2);
  }
  const options = { method: body === undefined ? 'GET' : 'POST', rejectUnauthorized: true,
    headers: { accept: 'application/json', 'x-oo-expected-publication-id': 'current' } };
  if (profile.ca_file) options.ca = readBounded(profile.ca_file, 262144);
  if (admin) {
    if (!profile.client_pem) refuse('ADMIN_IDENTITY_REQUIRED', 'Use --client-pem with an approved administrator mTLS identity.', 4);
    options.cert = options.key = readBounded(profile.client_pem, 262144);
  }
  if (profile.connect_ip) {
    options.lookup = (_hostname, opts, callback) => opts.all
      ? callback(null, [{ address: profile.connect_ip, family: isIP(profile.connect_ip) }])
      : callback(null, profile.connect_ip, isIP(profile.connect_ip));
  }
  if (profile.interface) {
    const addresses = networkInterfaces()[profile.interface];
    const family = isIP(profile.connect_ip || url.hostname) === 6 ? 'IPv6' : 'IPv4';
    options.localAddress = addresses?.find(address => address.family === family)?.address;
    if (!options.localAddress) refuse('INTERFACE_INVALID', 'No usable address on the configured network interface.');
  }
  if (token) options.headers.authorization = `Bearer ${token}`;
  const data = body === undefined ? undefined : form ? new URLSearchParams(body).toString() : JSON.stringify(body);
  if (data !== undefined) {
    options.headers['content-type'] = form ? 'application/x-www-form-urlencoded' : 'application/json';
    options.headers['content-length'] = Buffer.byteLength(data);
  }
  return new Promise((resolveResponse, reject) => {
    const failed = () => reject(new AuthError('GATEWAY_TRANSPORT', 'Gateway request failed; check TLS trust and connectivity.', 6));
    const req = request(url, options, response => {
      const chunks = [];
      let size = 0;
      response.on('data', chunk => {
        size += chunk.length;
        if (size > 4 * 1024 * 1024) req.destroy();
        else chunks.push(chunk);
      });
      response.on('error', failed);
      response.on('end', () => {
        clearTimeout(timer);
        try {
          const value = JSON.parse(Buffer.concat(chunks).toString('utf8'));
          if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error();
          resolveResponse({ status: response.statusCode, value });
        } catch { reject(new AuthError('GATEWAY_PROTOCOL', 'Gateway returned invalid JSON.', 7)); }
      });
    });
    const timer = setTimeout(() => req.destroy(), 15000);
    req.on('error', failed);
    req.on('close', () => clearTimeout(timer));
    req.end(data);
  });
}

function success(reply) {
  if (reply.status === 401 || reply.status === 403) {
    refuse('AUTH_RENEW_REQUIRED', 'Credential expired, revoked or refused. Ask an administrator to renew access, then run oo login again.', 4);
  }
  if (reply.status < 200 || reply.status >= 300 || reply.value.error) {
    refuse('GATEWAY_REFUSED', 'Gateway refused the request; no credential or remote error body was printed.', 8);
  }
  return reply.value;
}

function version(value) {
  if (typeof value !== 'string' || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(value)) {
    refuse('GATEWAY_PROTOCOL', 'Expected a stable semantic version.', 7);
  }
  const parts = value.split('.').map(Number);
  if (!parts.every(Number.isSafeInteger)) refuse('GATEWAY_PROTOCOL', 'Invalid semantic version.', 7);
  return parts;
}

export function checkVersion(meta, installed, packageName) {
  const current = version(installed);
  const minimum = version(meta.minimum_cli_version);
  const below = current.findIndex((part, i) => part !== minimum[i]);
  if (below !== -1 && current[below] < minimum[below]) {
    refuse('CLI_UPGRADE_REQUIRED',
      `Gateway requires CLI ${meta.minimum_cli_version}. Run: npm install -g ${packageName}@${meta.minimum_cli_version}`, 9);
  }
  if (meta.protocol_version !== '0.2') refuse('PROTOCOL_UNSUPPORTED', 'Unsupported Gateway protocol; upgrade the CLI.', 10);
}

export function cachePath(dir, profile) {
  const key = JSON.stringify([endpoint(profile.gateway, '/v1/meta').href, profile.connect_ip, profile.interface,
    profile.ca_file ? createHash('sha256').update(readBounded(profile.ca_file, 262144)).digest('hex') : null]);
  return join(dir, `meta-${createHash('sha256').update(key).digest('hex')}.json`);
}

export async function negotiate(context, profile, io = exchange, now = Date.now()) {
  const { dir, privatePath, atomicWrite, packageName, packageVersion } = context;
  const path = cachePath(dir, profile);
  let meta;
  try {
    privatePath(dir, true);
    privatePath(path);
    const cached = JSON.parse(readBounded(path, 16384).toString('utf8'));
    if (Number.isSafeInteger(cached.checked_at_ms) && now >= cached.checked_at_ms
        && now - cached.checked_at_ms < DAY) meta = cached.meta;
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (!meta) {
    const value = success(await io(profile, '/v1/meta'));
    // Cache only anonymous negotiation, never publication, identity or credentials.
    meta = { protocol_version: value.protocol_version, minimum_cli_version: value.minimum_cli_version };
    checkVersion(meta, packageVersion, packageName);
    mkdirSync(dir, { recursive: true, mode: 0o700 });
    privatePath(dir, true);
    atomicWrite(path, `${JSON.stringify({ checked_at_ms: now, meta })}\n`);
  }
  checkVersion(meta, packageVersion, packageName);
  return meta;
}

export function expiryWarning(identity, now = Date.now()) {
  const expiry = identity.expires_at_ms;
  if (!Number.isSafeInteger(expiry)) refuse('GATEWAY_PROTOCOL', 'Identity response has no valid expiry.', 7);
  if (expiry <= now) return 'Access has expired. Ask an administrator to renew access, then run oo login again.\n';
  if (expiry - now <= 7 * DAY) return 'Access expires within 7 days. Ask an administrator to renew access before expiry.\n';
  return '';
}

export async function pollDevice(profile, device, {
  io = exchange, wait = sleep, now = Date.now,
} = {}) {
  if (typeof device.device_code !== 'string' || !/^[a-f0-9]{64}$/.test(device.device_code)
      || !Number.isInteger(device.expires_in) || device.expires_in <= 0 || device.expires_in > 1800
      || (device.interval !== undefined && (!Number.isInteger(device.interval) || device.interval < 1 || device.interval > 300))) {
    refuse('GATEWAY_PROTOCOL', 'Invalid device authorization response.', 7);
  }
  let interval = (device.interval ?? 5) * 1000;
  const deadline = now() + device.expires_in * 1000;
  while (now() + interval < deadline) {
    await wait(interval);
    if (now() >= deadline) break;
    let reply;
    try {
      reply = await io(profile, '/v1/auth/device/token', { form: true,
        body: { client_id: CLIENT, grant_type: GRANT, device_code: device.device_code } });
    } catch (error) {
      if (error instanceof AuthError && error.code === 'GATEWAY_TRANSPORT') {
        interval *= 2;
        continue;
      }
      throw error;
    }
    if (reply.value.error === 'slow_down') interval += 5000;
    else if (reply.value.error !== 'authorization_pending') return success(reply).access_token;
  }
  refuse('DEVICE_EXPIRED', 'Device authorization expired. Run oo login --device again.', 4);
}

function options(args, allowed, switches = []) {
  const result = {};
  for (let i = 0; i < args.length; i++) {
    const flag = args[i];
    if (!allowed.includes(flag) || Object.hasOwn(result, flag)) refuse('AUTH_ARGUMENTS', 'Unknown or repeated option.', 2);
    result[flag] = switches.includes(flag) ? true : args[++i];
    if (result[flag] !== true && (typeof result[flag] !== 'string'
        || !result[flag] || result[flag].startsWith('-') || /[\x00-\x1f\x7f]/.test(result[flag]))) {
      refuse('AUTH_ARGUMENTS', 'Missing or invalid option value.', 2);
    }
  }
  return result;
}

const connectionFlags = ['--gateway', '--ca-file', '--connect-ip', '--interface', '--client-pem'];
function connection(context, opts, saved = true) {
  const profile = saved ? { ...context.readProfile(context.dir) } : { schema_version: 1 };
  const previous = profile.gateway;
  for (const flag of connectionFlags) {
    if (opts[flag]) profile[flag.slice(2).replaceAll('-', '_')] = opts[flag];
  }
  if (previous && context.endpointKey(previous) !== context.endpointKey(profile.gateway)) {
    // Never forward credentials, trust roots or routing from a different profile.
    for (const key of ['token_file', 'ca_file', 'client_pem', 'connect_ip', 'interface']) {
      if (!opts[`--${key.replaceAll('_', '-')}`]) delete profile[key];
    }
  }
  if (!context.endpointKey(profile.gateway)) refuse('GATEWAY_INVALID', 'Use --gateway with a valid HTTPS URL.', 2);
  endpoint(profile.gateway, '/v1/meta');
  for (const key of ['ca_file', 'client_pem']) if (profile[key]) profile[key] = resolve(profile[key]);
  if (profile.connect_ip && !isIP(profile.connect_ip)) refuse('AUTH_ARGUMENTS', 'Invalid --connect-ip.', 2);
  return profile;
}

function readToken(context, profile) {
  if (!profile.token_file) refuse('AUTH_REQUIRED', 'Run oo login or oo configure first.', 4);
  context.privatePath(profile.token_file);
  const token = readBounded(profile.token_file, 4098).toString('utf8').replace(/\r?\n$/, '');
  if (!tokenPattern.test(token) || token.length > 4096) refuse('AUTH_REQUIRED', 'Invalid token file; run oo login again.', 4);
  return token;
}

async function identity(context, profile, token) {
  const value = success(await exchange(profile, '/v1/auth/whoami', { token }));
  for (const key of ['principal_id', 'role', 'project_id']) {
    if (typeof value[key] !== 'string' || !/^[A-Za-z0-9_.:-]{1,128}$/.test(value[key])) {
      refuse('GATEWAY_PROTOCOL', 'Invalid identity response.', 7);
    }
  }
  if (value.protocol_version !== '0.2') refuse('PROTOCOL_UNSUPPORTED', 'Identity protocol is unsupported.', 10);
  const clean = Object.fromEntries(['principal_id', 'role', 'project_id', 'expires_at_ms', 'protocol_version'].map(k => [k, value[k]]));
  const warning = expiryWarning(clean);
  if (warning) process.stderr.write(warning);
  if (clean.expires_at_ms <= Date.now()) refuse('AUTH_RENEW_REQUIRED', 'Expired credential; renew access and run oo login again.', 4);
  return clean;
}

export async function login(context, args) {
  const opts = options(args, [...connectionFlags, '--code', '--code-stdin', '--device', '--profile', '--business-timezone', '--json'],
    ['--code-stdin', '--device', '--json']);
  if (!opts['--gateway'] || ['--code', '--code-stdin', '--device'].filter(k => opts[k]).length !== 1) {
    refuse('AUTH_ARGUMENTS', 'Use --gateway and exactly one of --code, --code-stdin or --device.', 2);
  }
  const name = opts['--profile'] ?? 'default';
  if (!/^[A-Za-z0-9_-]{1,64}$/.test(name)) refuse('AUTH_ARGUMENTS', 'Invalid profile name.', 2);
  const profile = connection(context, opts, false);
  if (opts['--business-timezone']) {
    context.timeContext(opts['--business-timezone']);
    profile.business_timezone = opts['--business-timezone'];
  }
  const tokens = join(context.dir, 'tokens');
  const path = join(tokens, `${name}.token`);
  if (existsSync(path)) refuse('PROFILE_EXISTS', 'This profile already has a token; choose a new --profile name. Existing credentials were not replaced.');
  await negotiate(context, profile);
  mkdirSync(tokens, { recursive: true, mode: 0o700 });
  context.privatePath(context.dir, true);
  context.privatePath(tokens, true);
  let token;
  if (opts['--device']) {
    const device = success(await exchange(profile, '/v1/auth/device/code', { body: { client_id: CLIENT }, form: true }));
    if (!/^[A-Z0-9]{4}-[A-Z0-9]{4}$/.test(device.user_code)
        || device.verification_uri !== endpoint(profile.gateway, '/v1/auth/device').href) {
      refuse('GATEWAY_PROTOCOL', 'Invalid device verification address or user code.', 7);
    }
    process.stderr.write(`Visit ${device.verification_uri}\nDevice code: ${device.user_code}\nAsk your administrator to approve this code and confirm the role.\n`);
    token = await pollDevice(profile, device);
  } else {
    const code = opts['--code'] ?? await context.stdinToken();
    if (!codePattern.test(code)) refuse('AUTH_ARGUMENTS', 'Invalid one-time enrollment code.', 2);
    token = success(await exchange(profile, '/v1/auth/enroll', { body: { code } })).token;
  }
  if (typeof token !== 'string' || !tokenPattern.test(token) || token.length > 4096) {
    refuse('GATEWAY_PROTOCOL', 'Enrollment returned an invalid client token.', 7);
  }
  // A redeemed code cannot be retried; retain its credential on verification failure.
  context.createToken(path, token);
  let who;
  try {
    who = await identity(context, profile, token);
    success(await exchange(profile, '/v1/tables', { token }));
  } catch (error) {
    refuse('LOGIN_VERIFICATION_FAILED', `Credential saved in tokens/${name}.token (0600); active profile unchanged. Configure the saved token file to retry verification or ask the administrator to revoke it; do not redeem the code again.`, error.exit ?? 1);
  }
  profile.token_file = path;
  profile.identity = who;
  context.validateProfile(profile);
  try { context.atomicWrite(join(context.dir, 'config.json'), `${JSON.stringify(profile, null, 2)}\n`); }
  catch {
    refuse('LOGIN_PROFILE_SAVE_FAILED', `Credential saved in tokens/${name}.token (0600); active profile unchanged. Configure the saved token file or ask the administrator to revoke it.`);
  }
  process.stdout.write(opts['--json'] ? `${JSON.stringify({ ...who, read_check: 'passed', profile: name })}\n`
    : `Logged in as ${who.principal_id} (${who.role}); read check passed.\n`);
  return 0;
}

export async function whoami(context, args) {
  const opts = options(args, [...connectionFlags, '--json'], ['--json']);
  const profile = connection(context, opts);
  await negotiate(context, profile);
  const who = await identity(context, profile, readToken(context, profile));
  process.stdout.write(opts['--json'] ? `${JSON.stringify(who)}\n`
    : `Identity: ${who.principal_id}\nRole: ${who.role}\nExpiry: ${new Date(who.expires_at_ms).toISOString()}\nProtocol: ${who.protocol_version}\n`);
  return 0;
}

function grantFile(context, opts, profile) {
  if (!opts['--grant-file']) refuse('AUTH_ARGUMENTS', 'Use --grant-file with the reviewed project, principal, role, policy and audience grant.', 2);
  const grant = JSON.parse(readBounded(opts['--grant-file'], 16384).toString('utf8'));
  if (!grant || typeof grant !== 'object' || Array.isArray(grant)) refuse('AUTH_ARGUMENTS', 'Invalid grant file.', 2);
  if (opts['--role']) grant.role = opts['--role'];
  if (!/^[A-Za-z0-9_.:-]{1,128}$/.test(grant.role)
      || context.endpointKey(grant.audience_url) !== context.endpointKey(profile.gateway)) {
    refuse('AUTH_ARGUMENTS', 'Grant role or audience does not match the selected Gateway.', 2);
  }
  return grant;
}

export async function approve(context, args) {
  const [userCode, ...rest] = args;
  if (!/^[A-Z0-9]{4}-[A-Z0-9]{4}$/.test(userCode ?? '')) refuse('AUTH_ARGUMENTS', 'Use oo token approve <user_code> --grant-file FILE.', 2);
  const opts = options(rest, [...connectionFlags, '--grant-file', '--role', '--json'], ['--json']);
  const profile = connection(context, opts);
  const grant = grantFile(context, opts, profile);
  await negotiate(context, profile);
  success(await exchange(profile, '/internal/v1/tokens', { admin: true, body: { operation: 'device_approve', user_code: userCode, grant } }));
  process.stdout.write(opts['--json'] ? '{"approved":true}\n' : 'Device approved for the reviewed grant.\n');
  return 0;
}

export function prompt({ packageName, packageVersion, gateway, material, raw, agent, expires }) {
  const lines = raw ? ['RISK: This prompt contains a long-lived bearer token. Anyone with the chat history can use its granted access. Do not share or log it.'] : [];
  lines.push('Configure Open Ontology read-only data access:',
    `1. Install: npm install -g ${packageName}@${packageVersion}`);
  if (raw) lines.push(`2. Bind: printf '%s\\n' ${shell(material)} | oo configure --gateway ${shell(gateway)} --token-stdin`);
  else lines.push(`2. Bind: printf '%s\\n' ${shell(material)} | oo login --gateway ${shell(gateway)} --code-stdin`,
    `   This code is single-use; expires at ${new Date(expires).toISOString()}. Do not put it in argv.`);
  lines.push('3. Verify: oo whoami && oo tables', '4. Read instructions: oo agent-skill',
    'Before querying, use oo describe to understand tables, entities and metric definitions.',
    'Report NULL, truncation and failed queries honestly; never substitute zero or invent results.');
  if (agent) {
    lines.push(`MCP setup for ${agent}: tools/mcp-gateway is a separate stdio executable (mcp-gateway).`,
      'Build the reviewed tools/mcp-gateway crate with cargo build --manifest-path tools/mcp-gateway/Cargo.toml --locked.',
      'Its current transport accepts only a loopback HTTP Gateway. It cannot connect directly to this HTTPS Gateway.',
      'Do not disable TLS or expose an unauthenticated relay. Have the administrator supply an approved loopback endpoint first.',
      'Set OO_MCP_GATEWAY_URL to that endpoint and OO_MCP_TOKEN_FILE to the token_file reference in your private config.json; never put token text in MCP configuration.');
    if (agent === 'claude') lines.push('Then register the absolute executable path with: claude mcp add --transport stdio open-ontology -- /absolute/path/to/mcp-gateway');
    if (agent === 'cursor') lines.push('Then add mcpServers.open-ontology to .cursor/mcp.json with command set to the absolute executable path and env containing the two references above.');
    if (agent === 'codex') lines.push('Then add [mcp_servers.open-ontology] in the Codex config with command set to the absolute executable path and its env table containing the two references above.');
  }
  return `${lines.join('\n')}\n`;
}

export async function sharePrompt(context, args) {
  const opts = options(args, [...connectionFlags, '--grant-file', '--role', '--agent', '--raw-token'], ['--raw-token']);
  if (opts['--agent'] && !['claude', 'cursor', 'codex'].includes(opts['--agent'])) refuse('AUTH_ARGUMENTS', 'Unknown agent.', 2);
  const profile = connection(context, opts);
  const grant = grantFile(context, opts, profile);
  await negotiate(context, profile);
  const raw = !!opts['--raw-token'];
  const issued = success(await exchange(profile, '/internal/v1/tokens', {
    admin: true, body: { operation: raw ? 'issue' : 'enroll', grant },
  }));
  const material = issued[raw ? 'token' : 'code'];
  if (typeof material !== 'string' || !(raw ? tokenPattern : codePattern).test(material)
      || !Number.isSafeInteger(issued.expires_at_ms) || issued.expires_at_ms <= Date.now()) {
    refuse('GATEWAY_PROTOCOL', 'Invalid issuance response.', 7);
  }
  process.stdout.write(prompt({ packageName: context.packageName, packageVersion: context.packageVersion,
    gateway: profile.gateway, material, raw, agent: opts['--agent'], expires: issued.expires_at_ms }));
  return 0;
}
