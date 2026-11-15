import { chmod, mkdir, readFile, realpath, stat, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const productRoot = resolve(packageRoot, '../..');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const targets = new Set(['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64', 'win32-x64']);

export function checkBinary(bytes, target) {
  if (!targets.has(target)) throw new Error('unsupported build target');
  const elf = bytes.length > 20 && bytes.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]));
  const macho = bytes.length > 8 && bytes.readUInt32LE(0) === 0xfeedfacf;
  if (target.startsWith('linux-')) {
    const machine = target.endsWith('arm64') ? 183 : 62;
    if (!elf || bytes[4] !== 2 || bytes[5] !== 1 || bytes.readUInt16LE(18) !== machine) {
      throw new Error('binary architecture does not match target');
    }
  } else if (target.startsWith('darwin-')) {
    const cpu = target.endsWith('arm64') ? 0x0100000c : 0x01000007;
    if (!macho || bytes.readUInt32LE(4) !== cpu) throw new Error('binary architecture does not match target');
  } else {
    if (bytes.length < 64 || bytes.toString('ascii', 0, 2) !== 'MZ') throw new Error('invalid Windows binary');
    const offset = bytes.readUInt32LE(60);
    if (offset > bytes.length - 6 || bytes.toString('ascii', offset, offset + 4) !== 'PE\0\0'
        || bytes.readUInt16LE(offset + 4) !== 0x8664) throw new Error('binary architecture does not match target');
  }
}

export function checkImage(value) {
  const image = value.trim();
  if (!/^(?:[a-z0-9][a-z0-9./:_-]*@)?sha256:[0-9a-f]{64}$/.test(image)) {
    throw new Error('lite image sidecar must contain a pinned content digest');
  }
  return image + '\n';
}

const hex64 = /^[0-9a-f]{64}$/;
const imageId = /^sha256:[0-9a-f]{64}$/;

// Same contract as the CLI's parser (crates/oo-cli/src/admin/lite_bundle.rs).
export function checkLiteBundles(text, image) {
  const fail = why => { throw new Error(`lite bundle manifest ${why}`); };
  let pin;
  try { pin = JSON.parse(text); } catch { fail('is not JSON'); }
  if (pin?.schema !== 'oo.lite-bundles/v1') fail('has the wrong schema');
  if (pin.runtime !== image.trim() || !image.includes('@sha256:')) fail('names a different runtime than oo-lite-image');
  const platforms = pin.platforms;
  if (!platforms || typeof platforms !== 'object' || !Object.keys(platforms).length) fail('has no platforms');
  const out = { schema: pin.schema, runtime: pin.runtime, platforms: {} };
  if (typeof pin.tag === 'string' && /^v[0-9A-Za-z.-]{1,32}$/.test(pin.tag)) out.tag = pin.tag;
  for (const [platform, row] of Object.entries(platforms)) {
    if (!['linux/amd64', 'linux/arm64'].includes(platform)) fail(`has unsupported platform ${platform}`);
    const { asset, url, size, sha256, manifest_sha256, image_id, config_id } = row ?? {};
    let parsed;
    try { parsed = new URL(url); } catch { fail(`has an invalid URL for ${platform}`); }
    if (typeof asset !== 'string' || !/^[A-Za-z0-9_-][A-Za-z0-9._-]{0,127}$/.test(asset)
        || parsed.protocol !== 'https:' || parsed.username || parsed.password || !url.endsWith('/' + asset)
        || !Number.isSafeInteger(size) || size <= 0 || size >= 2 ** 31
        || !hex64.test(sha256) || !hex64.test(manifest_sha256)
        || !imageId.test(image_id) || !imageId.test(config_id)) fail(`has an invalid ${platform} entry`);
    out.platforms[platform] = { asset, url, size, sha256, manifest_sha256, image_id, config_id };
  }
  return JSON.stringify(out, null, 2) + '\n';
}

async function outsideRepository(output) {
  const parent = await realpath(dirname(resolve(output)));
  for (let current = parent; ; current = dirname(current)) {
    try {
      await stat(join(current, '.git'));
      // An ignored artifact directory is also safe; never pack tracked source.
      try {
        const top = execFileSync('git', ['rev-parse', '--show-toplevel'], { cwd: current, encoding: 'utf8' }).trim();
        if (top !== current) throw new Error('not a repository');
        execFileSync('git', ['check-ignore', '--quiet', resolve(output)], { cwd: current });
        return;
      }
      catch { throw new Error('stage output must be outside Git or in a git-ignored directory'); }
    }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
    if (dirname(current) === current) break;
  }
}

export async function stage({ output, binaries, notices, root = packageRoot, partial = false, liteBundles }) {
  if (!isAbsolute(output) || !binaries.size || !notices) {
    throw new Error('absolute output, native binaries, and dependency notices required');
  }
  await outsideRepository(output);
  const files = ['package.json', 'README.md', 'bin/oo.mjs', 'bin/auth.mjs'];
  const sources = await Promise.all(files.map(file => readFile(join(root, file))));
  const metadata = JSON.parse(sources[0]);
  const declared = metadata.ooPlatforms;
  if (!Array.isArray(declared) || !declared.length || new Set(declared).size !== declared.length
      || declared.some(target => !targets.has(target))
      || [...binaries.keys()].some(target => !declared.includes(target))) {
    throw new Error('invalid declared platform set');
  }
  if (!partial && declared.some(target => !binaries.has(target))) {
    throw new Error('release staging requires every declared platform');
  }
  if (partial) {
    metadata.private = true;
    metadata.ooPlatforms = [...binaries.keys()].sort();
  }
  sources[0] = Buffer.from(JSON.stringify(metadata, null, 2) + '\n');
  const license = await readFile(join(productRoot, 'LICENSE'));
  const notice = await readFile(join(productRoot, 'NOTICE'));
  const skill = await readFile(join(productRoot, 'tools/agent-contract/business-answer-policy.md'));
  const thirdParty = await readFile(join(notices, 'THIRD_PARTY_NOTICES.txt'));
  const dependencies = await readFile(join(notices, 'dependencies.json'));
  const inventory = JSON.parse(dependencies);
  if (inventory.schema_version !== 1 || !Array.isArray(inventory.dependencies)
      || inventory.dependencies.some(item => !Array.isArray(item.unresolved) || item.unresolved.length)) {
    throw new Error('dependency source notice collection is incomplete');
  }
  if (!thirdParty.length) throw new Error('dependency notices must not be empty');
  const bundles = liteBundles ? await readFile(liteBundles, 'utf8') : undefined;
  const verified = [];
  for (const [target, path] of binaries) {
    const bytes = await readFile(path);
    checkBinary(bytes, target);
    const image = checkImage(await readFile(join(dirname(path), 'oo-lite-image'), 'utf8'));
    const bundle = bundles === undefined ? undefined : checkLiteBundles(bundles, image);
    let doctor;
    try {
      doctor = await readFile(join(dirname(path), 'oo-mysql-doctor'));
      checkBinary(doctor, target);
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    let mcp;
    try {
      mcp = await readFile(join(dirname(path), 'mcp-gateway'));
      checkBinary(mcp, target);
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    if (!partial && !mcp) throw new Error('release staging requires an MCP bridge for every target');
    if (!doctor) throw new Error('staging requires oo-mysql-doctor for every target');
    for (const [present, name] of [[mcp, 'mcp-gateway'], [doctor, 'oo-mysql-doctor']]) {
      if (present && !inventory.dependencies.some(item => item.name === name)) {
        throw new Error(`dependency notices missing for ${name}`);
      }
    }
    verified.push({ target, path, bytes, image, bundle, doctor, mcp });
  }
  // Construct an allowlisted clean directory; never npm-pack the repository.
  await mkdir(output, { mode: 0o700 });
  await mkdir(join(output, 'bin'));
  for (let i = 0; i < files.length; i++) await writeFile(join(output, files[i]), sources[i]);
  await chmod(join(output, 'bin/oo.mjs'), 0o755);
  await writeFile(join(output, 'LICENSE'), license);
  await writeFile(join(output, 'NOTICE'), notice);
  await writeFile(join(output, 'THIRD_PARTY_NOTICES.txt'), thirdParty);
  await writeFile(join(output, 'dependencies.json'), dependencies);
  await writeFile(join(output, 'SKILL.md'), skill);
  const manifest = { schema_version: 1, skill_sha256: sha256(skill), binaries: {} };
  for (const { target, bytes, image, bundle, doctor, mcp } of verified) {
    const relative = `native/${target}/oo${target.startsWith('win32-') ? '.exe' : ''}`;
    await mkdir(dirname(join(output, relative)), { recursive: true });
    await writeFile(join(output, relative), bytes);
    await chmod(join(output, relative), 0o755);
    await writeFile(join(dirname(join(output, relative)), 'oo-lite-image'), image);
    if (bundle) await writeFile(join(dirname(join(output, relative)), 'oo-lite-bundle.json'), bundle);
    manifest.binaries[target] = { path: relative, sha256: sha256(bytes) };
    if (mcp) {
      const path = `native/${target}/mcp-gateway${target.startsWith('win32-') ? '.exe' : ''}`;
      await writeFile(join(output, path), mcp, { mode: 0o755 });
      manifest.binaries[target].mcp_gateway = { path, sha256: sha256(mcp) };
    }
    if (doctor) {
      const path = `native/${target}/oo-mysql-doctor${target.startsWith('win32-') ? '.exe' : ''}`;
      await writeFile(join(output, path), doctor, { mode: 0o755 });
      manifest.binaries[target].mysql_doctor = { path, sha256: sha256(doctor) };
    }
  }
  await writeFile(join(output, 'native/manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
  return { output, targets: Object.keys(manifest.binaries), skill_sha256: manifest.skill_sha256 };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    let output, notices, liteBundles, partial = false;
    const binaries = new Map();
    for (let i = 2; i < process.argv.length; i += 2) {
      const [flag, value] = process.argv.slice(i, i + 2);
      if (flag === '--output' && value && !output) output = value;
      else if (flag === '--notices' && value && !notices) notices = value;
      else if (flag === '--partial' && value === 'true') partial = true;
      else if (flag === '--lite-bundles' && value && !liteBundles) liteBundles = value;
      else if (flag === '--binary' && value?.includes('=')) {
        const equal = value.indexOf('=');
        const target = value.slice(0, equal);
        if (binaries.has(target)) throw new Error('duplicate build target');
        binaries.set(target, value.slice(equal + 1));
      } else throw new Error('usage: stage.mjs --output /absolute/new-directory --notices /path/to/notices [--lite-bundles lite-bundles.json] --binary platform-arch=/path/to/oo');
    }
    console.log(JSON.stringify(await stage({ output, binaries, notices, partial, liteBundles })));
  } catch (error) {
    console.error(`CLI package staging failed: ${error.message}`);
    process.exitCode = 1;
  }
}
