#!/usr/bin/env node
import { createHash, randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import {
  accessSync, chmodSync, closeSync, constants, existsSync, fchmodSync, fsyncSync,
  lstatSync, mkdirSync, openSync, readFileSync, realpathSync, renameSync, statSync,
  unlinkSync, writeFileSync,
} from 'node:fs';
import { isIP } from 'node:net';
import { constants as osConstants, homedir } from 'node:os';
import { dirname, isAbsolute, join, parse, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { AuthError, login, whoami, sharePrompt, approve, negotiate } from './auth.mjs';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const fields = {
  gateway: '--gateway', token_file: '--token-file', connect_ip: '--connect-ip',
  interface: '--interface', ca_file: '--ca-file', client_pem: '--client-pem',
};
const connectedCommands = new Set([
  'query', 'tables', 'describe', 'validate', 'sql', 'freshness',
  'knowledge', 'cancel', 'status', 'operations',
]);
class CliError extends Error {}
function fail(code, message) { throw new CliError(`${code}: ${message}`); }
function object(value) { return value !== null && typeof value === 'object' && !Array.isArray(value); }
function text(value, max = 4096) {
  return typeof value === 'string' && value.length > 0
    && Buffer.byteLength(value) <= max && !/[\x00-\x1f\x7f]/.test(value);
}

export function timeContext(timezone, at = new Date().toISOString()) {
  if (!text(timezone, 128) || !/^(?:UTC|[A-Za-z_]+(?:\/[A-Za-z0-9_+-]+)+)$/.test(timezone)) {
    fail('TIMEZONE_REQUIRED', 'Use an explicit IANA business timezone, e.g. Asia/Shanghai; the host timezone is not a business calendar.');
  }
  if (typeof at !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,3})?(?:Z|[+-]\d{2}:\d{2})$/.test(at)) {
    fail('TIME_INVALID', '--at must be an RFC3339 instant with Z or a numeric offset.');
  }
  const instant = new Date(at);
  const [year, month, day] = at.slice(0, 10).split('-').map(Number);
  const days = new Date(Date.UTC(year, month, 0)).getUTCDate();
  if (!Number.isFinite(instant.getTime()) || year < 100 || month < 1 || month > 12 || day < 1 || day > days
      || Number(at.slice(11, 13)) > 23 || Number(at.slice(14, 16)) > 59 || Number(at.slice(17, 19)) > 59) {
    fail('TIME_INVALID', 'Invalid calendar date or instant.');
  }
  let parts;
  try {
    parts = Object.fromEntries(new Intl.DateTimeFormat('en-CA', {
      timeZone: timezone, year: 'numeric', month: '2-digit', day: '2-digit',
    }).formatToParts(instant).map(part => [part.type, part.value]));
  } catch { fail('TIMEZONE_INVALID', 'Unknown IANA timezone.'); }
  const today = `${parts.year}-${parts.month}-${parts.day}`;
  const yesterday = new Date(Date.parse(`${today}T00:00:00Z`) - 86400000).toISOString().slice(0, 10);
  return {
    timezone, evaluated_at: instant.toISOString(), today,
    yesterday: { start: yesterday, end_exclusive: today, basis: 'local_civil_day' },
    previous_business_day: null, business_calendar: 'not_configured',
    scope: 'Query date context only; not source freshness or an approved business-day/SLA calendar.',
  };
}

export function nativeBinary(root = packageRoot, platform = process.platform, arch = process.arch) {
  const target = `${platform}-${arch}`;
  let manifest, base;
  try {
    base = realpathSync(root);
    const path = join(base, 'native', 'manifest.json');
    if (!lstatSync(path).isFile() || realpathSync(path) !== path) throw new Error();
    manifest = JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    fail('NATIVE_MANIFEST_INVALID', 'Packaged native/manifest.json is missing or invalid; reinstall the package.');
  }
  if (!object(manifest) || manifest.schema_version !== 1 || !object(manifest.binaries)
      || Object.keys(manifest.binaries).some(key => !/^[a-z0-9]+-[a-z0-9_]+$/.test(key))) {
    fail('NATIVE_MANIFEST_INVALID', 'Expected schema_version 1 and a binaries map.');
  }
  if (!Object.hasOwn(manifest.binaries, target)) {
    fail('UNSUPPORTED_PLATFORM', `${target} is not packaged. Available: ${Object.keys(manifest.binaries).join(', ') || 'none'}.`);
  }
  const entry = manifest.binaries[target];
  const relative = `native/${target}/oo${platform === 'win32' ? '.exe' : ''}`;
  if (!object(entry) || entry.path !== relative || typeof entry.sha256 !== 'string'
      || !/^[a-f0-9]{64}$/i.test(entry.sha256)) {
    fail('NATIVE_MANIFEST_INVALID', 'The selected executable path or SHA256 is invalid.');
  }
  const binary = join(base, relative);
  let bytes;
  try {
    if (!lstatSync(binary).isFile() || realpathSync(binary) !== binary) throw new Error();
    bytes = readFileSync(binary);
  } catch {
    fail('NATIVE_BINARY_MISSING', 'The packaged executable is missing or is a symlink; reinstall the package.');
  }
  if (createHash('sha256').update(bytes).digest('hex') !== entry.sha256.toLowerCase()) {
    fail('NATIVE_DIGEST_MISMATCH', 'Packaged executable failed SHA256 verification; reinstall the package.');
  }
  try { accessSync(binary, constants.X_OK); } catch {
    fail('NATIVE_NOT_EXECUTABLE', 'The packaged native executable is not executable.');
  }
  return binary;
}

function configDirectory() {
  const value = process.env.OO_CONFIG_DIR ?? join(homedir(), '.config', 'open-ontology');
  if (!text(value) || resolve(value) === parse(resolve(value)).root) {
    fail('CONFIG_DIR_INVALID', 'OO_CONFIG_DIR must name a private configuration directory.');
  }
  return resolve(value);
}

function privatePath(path, directory = false) {
  const info = lstatSync(path);
  if (!(directory ? info.isDirectory() : info.isFile()) || info.isSymbolicLink()
      || (process.platform !== 'win32'
          && ((info.mode & 0o077) !== 0 || info.uid !== process.getuid()))) {
    fail('CONFIG_PERMISSIONS', 'Configuration must be owner-only and must not be a symlink.');
  }
}

function endpointKey(value, allowHttp = false) {
  if (!text(value, 2048) || !/^https?:\/\//i.test(value)
      || /[^\x21-\x7e]|[%\\@?#]/.test(value)) return null;
  try {
    const url = new URL(value);
    const path = value.slice(value.indexOf('://') + 3).replace(/^[^/]*/, '') || '/';
    if (!url.hostname || (url.protocol !== 'https:' && !(allowHttp && url.protocol === 'http:'))
        || url.username || url.password || path !== url.pathname) return null;
    return `${url.origin}${url.pathname.replace(/\/$/, '')}`;
  } catch { return null; }
}

function validateProfile(profile) {
  if (!object(profile) || profile.schema_version !== 1
      || Object.keys(profile).some(key => !['schema_version', 'business_timezone', 'identity'].includes(key) && !Object.hasOwn(fields, key))
      || !endpointKey(profile.gateway) || !text(profile.token_file) || !isAbsolute(profile.token_file)) {
    fail('CONFIG_INVALID', 'Expected a valid HTTPS gateway and an absolute token-file reference; run oo configure.');
  }
  for (const key of ['ca_file', 'client_pem']) {
    if (Object.hasOwn(profile, key) && (!text(profile[key]) || !isAbsolute(profile[key]))) {
      fail('CONFIG_INVALID', 'Certificate references must be absolute file paths.');
    }
  }
  if (Object.hasOwn(profile, 'connect_ip') && (!text(profile.connect_ip, 45) || !isIP(profile.connect_ip))) {
    fail('CONFIG_INVALID', 'connect-ip must be an IPv4 or IPv6 address without a port.');
  }
  if (profile.connect_ip && isIP(new URL(profile.gateway).hostname.replace(/^\[|\]$/g, ''))) {
    fail('CONFIG_INVALID', 'connect-ip requires a gateway DNS hostname.');
  }
  if (Object.hasOwn(profile, 'interface')
      && (!text(profile.interface, 255) || /[^\x21-\x7e]/.test(profile.interface))) {
    fail('CONFIG_INVALID', 'interface must be printable ASCII without whitespace.');
  }
  if (Object.hasOwn(profile, 'business_timezone')) timeContext(profile.business_timezone);
  if (Object.hasOwn(profile, 'identity') && (!object(profile.identity)
      || Object.keys(profile.identity).some(key => !['principal_id', 'project_id', 'role', 'expires_at_ms', 'protocol_version'].includes(key)))) {
    fail('CONFIG_INVALID', 'Invalid saved identity metadata.');
  }
  return profile;
}

function readProfile(dir) {
  try {
    privatePath(dir, true);
    const path = join(dir, 'config.json');
    privatePath(path);
    if (statSync(path).size > 16384) throw new Error();
    return validateProfile(JSON.parse(readFileSync(path, 'utf8')));
  } catch (error) {
    if (error.code === 'ENOENT') return null;
    if (error instanceof CliError) throw error;
    fail('CONFIG_INVALID', 'Cannot read configuration; run oo configure to replace it.');
  }
}

function atomicWrite(path, data) {
  const temporary = `${path}.${randomUUID()}.tmp`;
  let fd;
  try {
    fd = openSync(temporary, 'wx', 0o600);
    fchmodSync(fd, 0o600);
    writeFileSync(fd, data);
    fsyncSync(fd);
    closeSync(fd);
    fd = undefined;
    renameSync(temporary, path);
  } finally {
    if (fd !== undefined) closeSync(fd);
    if (existsSync(temporary)) unlinkSync(temporary);
  }
}

async function stdinToken() {
  if (process.stdin.isTTY) fail('TOKEN_INPUT_INVALID', 'Redirect or pipe the token to stdin; do not enter it on a visible terminal.');
  const chunks = [];
  let size = 0;
  for await (const chunk of process.stdin) {
    size += chunk.length;
    if (size > 4098) fail('TOKEN_INPUT_INVALID', 'Token input exceeds 4096 bytes plus a line ending.');
    chunks.push(chunk);
  }
  const token = Buffer.concat(chunks).toString('utf8').replace(/\r?\n$/, '');
  if (!/^[\x21-\x7e]{1,4096}$/.test(token)) {
    fail('TOKEN_INPUT_INVALID', 'Token must be a single nonempty printable ASCII value.');
  }
  return token;
}

async function configure(args) {
  const dir = configDirectory();
  const options = new Map();
  const allowed = new Set([...Object.values(fields), '--token-stdin', '--business-timezone']);
  for (let i = 0; i < args.length; i++) {
    const flag = args[i];
    if (!allowed.has(flag) || options.has(flag)) fail('CONFIG_ARGUMENTS', 'Unknown or repeated configure option.');
    const value = flag === '--token-stdin' ? true : args[++i];
    if (value !== true && (!text(value) || value.startsWith('-'))) fail('CONFIG_ARGUMENTS', 'Missing configure option value.');
    options.set(flag, value);
  }
  if (!options.has('--gateway') || options.has('--token-file') === options.has('--token-stdin')) {
    fail('CONFIG_ARGUMENTS', 'Use --gateway and exactly one of --token-stdin or --token-file.');
  }
  const profile = { schema_version: 1 };
  if (options.has('--business-timezone')) profile.business_timezone = options.get('--business-timezone');
  for (const [key, flag] of Object.entries(fields)) {
    if (options.has(flag)) profile[key] = options.get(flag);
  }
  for (const key of ['token_file', 'ca_file', 'client_pem']) {
    if (!profile[key]) continue;
    profile[key] = resolve(profile[key]);
    try {
      if (!statSync(profile[key]).isFile() || profile[key] === join(dir, 'config.json')) throw new Error();
      accessSync(profile[key], constants.R_OK);
    } catch { fail('CONFIG_FILE_INVALID', 'A configured credential or certificate file is not a readable file.'); }
  }
  const managedToken = options.has('--token-stdin') ? join(dir, `token-${randomUUID()}`) : null;
  if (managedToken) profile.token_file = managedToken;
  validateProfile(profile);
  const token = managedToken ? await stdinToken() : null;
  mkdirSync(dir, { recursive: true, mode: 0o700 });
  // Do not chmod through a user-supplied symlink or take ownership of another user's directory.
  const info = lstatSync(dir);
  if (!info.isDirectory() || info.isSymbolicLink()
      || (process.platform !== 'win32' && info.uid !== process.getuid())) {
    fail('CONFIG_PERMISSIONS', 'Configuration directory must be owned by this user and not be a symlink.');
  }
  chmodSync(dir, 0o700);
  try {
    // Immutable token files keep old readers consistent while config.json switches atomically.
    if (managedToken) atomicWrite(managedToken, token);
    atomicWrite(join(dir, 'config.json'), `${JSON.stringify(profile, null, 2)}\n`);
  } catch (error) {
    if (managedToken && existsSync(managedToken)) unlinkSync(managedToken);
    throw error;
  }
  process.stdout.write('Configured. Credentials are stored locally and are not displayed.\n'
    + 'Next: ontology agent-skill, then ontology freshness --summary --json.\n'
    + 'Use ontology for the oo examples if an older global oo is on PATH.\n');
}

export function nativeArgs(args, profile) {
  args = expandLongOptions(args);
  if (!profile) return args;
  const separator = args.indexOf('--');
  const head = separator < 0 ? args : args.slice(0, separator);
  if (head.filter(arg => arg === '--gateway' || arg.startsWith('--gateway=')).length > 1) {
    fail('GATEWAY_INVALID', 'Repeated --gateway options are not allowed.');
  }
  const explicit = new Set(head.map(arg => arg.split('=')[0]));
  const defaults = { ...profile, transport: 'production', auth: 'external' };
  if (explicit.has('--gateway')) {
    const index = head.indexOf('--gateway');
    const endpoint = index < 0 ? null : endpointKey(head[index + 1], true);
    if (!endpoint) fail('GATEWAY_INVALID', 'Use a valid gateway URL without credentials, query or fragment.');
    if (endpoint !== endpointKey(profile.gateway)) {
      if (!explicit.has('--token-file') && !explicit.has('--token-stdin')) {
        fail('GATEWAY_CREDENTIAL_REQUIRED', 'A different gateway requires explicit --token-file or --token-stdin.');
      }
      for (const key of ['token_file', 'ca_file', 'client_pem', 'connect_ip', 'interface']) delete defaults[key];
    }
  }
  if (explicit.has('--token-stdin') || (explicit.has('--auth')
      && head[head.indexOf('--auth') + 1] !== 'external')) delete defaults.token_file;
  const extras = [];
  for (const [key, flag] of Object.entries({ ...fields, transport: '--transport', auth: '--auth' })) {
    if (defaults[key] !== undefined && !explicit.has(flag)) extras.push(flag, defaults[key]);
  }
  return separator < 0 ? [...args, ...extras] : [...head, ...extras, ...args.slice(separator)];
}

export function expandLongOptions(args) {
  const values = new Set([...Object.values(fields), '--business-timezone', '--timezone', '--at',
    '--transport', '--auth', '--max-rows', '--timeout-ms', '--path-prefix', '--offset', '--limit',
    '--expected-publication-id', '--expected-data-vector-sha256',
    '--code', '--profile', '--role', '--agent', '--grant-file']);
  const result = [];
  let positional = false;
  for (const arg of args) {
    if (arg === '--') positional = true;
    const equal = arg.indexOf('=');
    if (!positional && equal > 0 && values.has(arg.slice(0, equal))) {
      result.push(arg.slice(0, equal), arg.slice(equal + 1));
    } else result.push(arg);
  }
  return result;
}

function delegate(binary, args, businessTimezone = null) {
  return new Promise((resolveExit, reject) => {
    const enrich = args[0] === 'freshness' && args.includes('--json') && businessTimezone;
    const child = spawn(binary, args, { stdio: enrich ? ['inherit', 'pipe', 'inherit'] : 'inherit', shell: false });
    const chunks = [];
    let bytes = 0;
    let passthrough = false;
    if (enrich) child.stdout.on('data', chunk => {
      bytes += chunk.length;
      if (passthrough) process.stdout.write(chunk);
      else if (bytes > 16 * 1024 * 1024) {
        passthrough = true;
        for (const saved of chunks) process.stdout.write(saved);
        chunks.length = 0;
        process.stdout.write(chunk);
      } else chunks.push(chunk);
    });
    const forwards = new Map(['SIGINT', 'SIGTERM', 'SIGHUP'].map(signal => [signal, () => child.kill(signal)]));
    for (const [signal, handler] of forwards) process.on(signal, handler);
    const cleanup = () => { for (const [signal, handler] of forwards) process.off(signal, handler); };
    child.once('error', () => {
      cleanup();
      reject(new CliError('NATIVE_EXECUTION_FAILED: Could not start the packaged executable.'));
    });
    child.once('close', (code, signal) => {
      cleanup();
      if (enrich && !passthrough) {
        const raw = Buffer.concat(chunks);
        try {
          const value = JSON.parse(raw);
          if (code === 0 && object(value) && !value.error && !value.code) {
            value.business_time = timeContext(businessTimezone);
            process.stdout.write(`${JSON.stringify(value)}\n`);
          } else process.stdout.write(raw);
        } catch { process.stdout.write(raw); }
      }
      resolveExit(code ?? (128 + (osConstants.signals[signal] ?? 1)));
    });
  });
}

async function main(args) {
  args = expandLongOptions(args);
  if (args[0] === 'help') {
    if (args.length > 3 || args.slice(1).some(arg => !/^[a-z][a-z0-9-]*$/.test(arg))) {
      fail('CONFIG_ARGUMENTS', 'Use oo help [command [subcommand]].');
    }
    args = args.length === 1 ? ['--help'] : [...args.slice(1), '--help'];
  }
  if (args[0] === '--version' || args[0] === '-V') {
    if (args.length > 2 || (args.length === 2 && args[1] !== '--json')) {
      fail('CONFIG_ARGUMENTS', 'Use ontology --version [--json].');
    }
    const { name, version } = JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8'));
    process.stdout.write(args[1] === '--json'
      ? `${JSON.stringify({ name, version })}\n` : `${name} ${version}\n`);
    return 0;
  }
  const authCommand = ['login', 'whoami', 'share-prompt'].includes(args[0])
    || (args[0] === 'token' && args[1] === 'approve');
  if (authCommand) {
    if (args.includes('--help') || args.includes('-h')) {
      process.stdout.write('Usage:\n'
        + '  oo login --gateway URL (--code CODE | --code-stdin | --device)\n'
        + '    [--profile NAME] [--business-timezone ZONE] [--json]\n'
        + '  oo whoami [--json]\n'
        + '  oo share-prompt --grant-file FILE [--role R] [--agent claude|cursor|codex] [--raw-token]\n'
        + '  oo token approve CODE --grant-file FILE [--role R] [--json]\n'
        + 'Connection options: --gateway URL --ca-file FILE --connect-ip IP --interface NAME\n'
        + 'Admin operations require --client-pem FILE (approved mTLS identity).\n'
        + 'Grant files contain reviewed project/principal/role/policy/knowledge/audience fields, never credentials.\n'
        + 'TLS verification cannot be disabled. Prefer --code-stdin; raw tokens never go in argv.\n');
      return 0;
    }
    const context = authContext();
    if (args[0] === 'login') return login(context, args.slice(1));
    if (args[0] === 'whoami') return whoami(context, args.slice(1));
    if (args[0] === 'share-prompt') return sharePrompt(context, args.slice(1));
    return approve(context, args.slice(2));
  }
  if (args[0] === 'configure') {
    if (args.includes('--help') || args.includes('-h')) {
      process.stdout.write('Usage: oo configure --gateway https://host (--token-stdin | --token-file FILE)\n'
        + '  [--connect-ip IP] [--interface NAME] [--ca-file FILE] [--client-pem FILE]\n'
        + '  [--business-timezone Asia/Shanghai]\n\n'
        + 'Save an HTTPS Gateway profile once, then run oo tables, describe, validate or query.\n'
        + '--token-file references an existing owner-only file; --token-stdin stores a private copy.\n'
        + 'Never put the token itself on the command line. Configuring does not contact the Gateway.\n'
        + 'Profiles default to ~/.config/open-ontology; OO_CONFIG_DIR selects another private directory.\n'
        + 'Changing the Gateway requires its own explicit token; credentials are never silently reused.\n');
    } else await configure(args.slice(1));
    return 0;
  }
  if (args[0] === 'time') {
    if (args.includes('--help') || args.includes('-h')) {
      process.stdout.write('Usage: oo time --timezone <IANA> [--at <RFC3339>] [--json]\n\n'
        + 'Resolve today/yesterday in an explicit business timezone without querying data.\n'
        + 'Without --timezone, use the configured --business-timezone; never guess from the host.\n'
        + 'This is a civil-day interval, not a previous-business-day, SLA or source-freshness claim.\n');
      return 0;
    }
    const options = new Map();
    for (let i = 1; i < args.length; i++) {
      const flag = args[i];
      if (!['--timezone', '--at', '--json'].includes(flag) || options.has(flag)) {
        fail('TIME_ARGUMENTS', 'Use oo time --help.');
      }
      const value = flag === '--json' ? true : args[++i];
      if (value !== true && (!value || value.startsWith('-'))) fail('TIME_ARGUMENTS', 'Missing time option value.');
      options.set(flag, value);
    }
    const timezone = options.get('--timezone') ?? readProfile(configDirectory())?.business_timezone;
    const context = timeContext(timezone, options.get('--at'));
    process.stdout.write(options.has('--json') ? `${JSON.stringify(context)}\n`
      : `Timezone: ${context.timezone}\nToday: ${context.today}\nYesterday: [${context.yesterday.start}, ${context.yesterday.end_exclusive})\nBusiness calendar: not configured (civil dates only)\n`);
    return 0;
  }
  if (args[0] === 'config' && (args.length === 1
      || args[1] === '--help' || args[1] === '-h'
      || (['show', 'set'].includes(args[1]) && (args[2] === '--help' || args[2] === '-h')))) {
    process.stdout.write('Usage: oo config show [--json]\n\n'
      + '       oo config set --business-timezone <IANA> [--json]\n\n'
      + 'Show the configured Gateway and credential-presence flags; never prints secrets or token paths.\n'
      + 'config set changes only the business timezone; credentials and routing stay unchanged.\n'
      + 'Use oo configure --help to create or replace the profile.\n'
      + 'OO_CONFIG_DIR selects the private profile directory (default ~/.config/open-ontology).\n');
    return 0;
  }
  if (args[0] === 'config' && args[1] === 'set') {
    const rest = args.slice(2).filter(arg => arg !== '--json');
    if (args.filter(arg => arg === '--json').length > 1 || rest.length !== 2
        || rest[0] !== '--business-timezone') fail('CONFIG_ARGUMENTS', 'Use oo config set --business-timezone <IANA> [--json].');
    timeContext(rest[1]);
    const dir = configDirectory();
    const profile = readProfile(dir);
    if (!profile) fail('CONFIG_INVALID', 'Configure a Gateway profile before changing its business timezone.');
    profile.business_timezone = rest[1];
    atomicWrite(join(dir, 'config.json'), `${JSON.stringify(profile, null, 2)}\n`);
    process.stdout.write(args.includes('--json')
      ? `${JSON.stringify({ configured: true, business_timezone: profile.business_timezone, credentials_changed: false })}\n`
      : `Business timezone: ${profile.business_timezone}. Credentials and routing are unchanged.\n`);
    return 0;
  }
  if (args[0] === 'config' && args[1] === 'show') {
    if (args.length > 3 || (args.length === 3 && args[2] !== '--json')) fail('CONFIG_ARGUMENTS', 'Use oo config show --json.');
    const profile = readProfile(configDirectory());
    process.stdout.write(`${JSON.stringify(profile ? {
      configured: true, gateway: profile.gateway, transport: 'production', auth: 'external',
      token_configured: true, connect_ip: profile.connect_ip, interface: profile.interface,
      business_timezone: profile.business_timezone,
      ca_file_configured: !!profile.ca_file, client_pem_configured: !!profile.client_pem,
    } : { configured: false }, null, 2)}\n`);
    return 0;
  }
  const beforeSeparator = args.includes('--') ? args.slice(0, args.indexOf('--')) : args;
  const offline = args[0] === 'agent-skill' || beforeSeparator.some(arg => arg === '--help' || arg === '-h');
  const binary = nativeBinary();
  const adminStatus = args[0] === 'status' && (args.length === 1
    || args.slice(1).some(arg => arg === '--config' || arg.startsWith('--config=') || arg === '--yes')
    || args.slice(1).every(arg => arg === '--json'));
  const profile = !offline && !adminStatus && connectedCommands.has(args[0]) ? readProfile(configDirectory()) : null;
  if (args.length === 1 && (args[0] === '--help' || args[0] === '-h')) {
    process.stdout.write('Open Ontology npm wrapper (oo and ontology are aliases):\n'
      + '  oo configure --gateway https://host (--token-stdin | --token-file FILE)\n'
      + '  oo config show --json\n'
      + '  oo config set --business-timezone Asia/Shanghai\n'
      + '  oo time --timezone Asia/Shanghai [--at RFC3339] [--json]\n'
      + '  oo --version [--json]\n'
      + '  oo agent-skill [--json]\n\n'
      + '  oo login --gateway URL (--code-stdin | --device)\n'
      + '  oo whoami [--json]\n'
      + '  oo share-prompt --grant-file FILE [--agent claude|cursor|codex]\n\n'
      + 'Quick start (no Skill, model, database driver or repository required):\n'
      + '  oo configure --gateway https://gateway.example --token-file /absolute/path/to/token\n'
      + '  oo tables\n'
      + '  oo describe app.orders\n'
      + '  oo validate "SELECT COUNT(*) FROM app.orders"\n'
      + '  oo query "SELECT COUNT(*) FROM app.orders"\n'
      + '  oo freshness --summary\n\n'
      + 'Use oo help <command> or oo <command> --help for details.\n'
      + 'OO_CONFIG_DIR selects a private profile; explicit connection flags override saved values.\n'
      + 'configure and config show are supplied by this package. Data commands below use the saved profile.\n\n');
  }
  const forwarded = nativeArgs(args, profile);
  const head = forwarded.includes('--') ? forwarded.slice(0, forwarded.indexOf('--')) : forwarded;
  const gatewayIndex = head.indexOf('--gateway');
  if (!offline && !adminStatus && connectedCommands.has(args[0]) && gatewayIndex >= 0
      && head[head.indexOf('--transport') + 1] !== 'local-development') {
    const selected = {};
    for (const [key, flag] of Object.entries(fields)) {
      const index = head.indexOf(flag);
      if (index >= 0) selected[key] = head[index + 1];
    }
    if (!endpointKey(selected.gateway)) fail('GATEWAY_INVALID', 'A valid HTTPS Gateway is required.');
    await negotiate(authContext(), selected);
  }
  const timezone = profile && gatewayIndex >= 0
    && endpointKey(head[gatewayIndex + 1], true) === endpointKey(profile.gateway)
    ? profile.business_timezone : undefined;
  return delegate(binary, forwarded, timezone);
}

function authContext() {
  const { name, version } = JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8'));
  return { dir: configDirectory(), packageName: name, packageVersion: version,
    privatePath, atomicWrite, readProfile, validateProfile, endpointKey, stdinToken, timeContext,
    createToken(path, token) {
      const fd = openSync(path, 'wx', 0o600);
      try { fchmodSync(fd, 0o600); writeFileSync(fd, token); fsyncSync(fd); }
      finally { closeSync(fd); }
    },
  };
}

function wrapperFailure(error) {
  if (error instanceof AuthError) return {
    class: error.exit === 2 ? 'usage' : error.exit === 4 ? 'auth' : 'local',
    code: error.code, message: error.message, exit: error.exit, retryable: false,
  };
  const match = error instanceof CliError && /^([A-Z][A-Z0-9_]+): (.*)$/s.exec(error.message);
  const code = match ? match[1] : 'LOCAL_FAILURE';
  const message = match ? match[2] : 'Unable to read or update local CLI files.';
  const usage = ['CONFIG_ARGUMENTS', 'TIME_ARGUMENTS', 'TIMEZONE_REQUIRED', 'TIMEZONE_INVALID',
    'TIME_INVALID', 'GATEWAY_INVALID'].includes(code);
  const auth = ['TOKEN_INPUT_INVALID', 'GATEWAY_CREDENTIAL_REQUIRED'].includes(code);
  const hint = code.startsWith('TIME')
    ? 'Run oo time --help; use an explicit IANA timezone and an RFC3339 instant with an offset.'
    : code.startsWith('NATIVE') || code === 'UNSUPPORTED_PLATFORM'
      ? 'Check oo --version and reinstall the package for this platform; no unverified global binary is used.'
      : 'Run oo configure --help or oo config show --json. Keep credentials in a private file or stdin, never in a command argument.';
  return { class: usage ? 'usage' : auth ? 'auth' : 'local', code,
    exit: usage ? 2 : auth ? 4 : 1, message, hint, retryable: false,
    retry: { outcome: 'not_applicable', nested_cli_retries: 0, attempt_count: 0,
      max_extra_attempts: 0, max_wait_ms: 0 } };
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { process.exitCode = await main(process.argv.slice(2)); }
  catch (error) {
    const failure = wrapperFailure(error);
    const args = process.argv.slice(2);
    const separator = args.indexOf('--');
    const head = separator < 0 ? args : args.slice(0, separator);
    if (head.includes('--json')) process.stdout.write(`${JSON.stringify(failure)}\n`);
    else process.stderr.write(`oo: ${failure.code}: ${failure.message}\nhint: ${failure.hint}\n`);
    process.exitCode = failure.exit;
  }
}
