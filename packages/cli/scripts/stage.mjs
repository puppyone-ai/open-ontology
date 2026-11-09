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

export async function stage({ output, binaries, notices, root = packageRoot, partial = false }) {
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
  const verified = [];
  for (const [target, path] of binaries) {
    const bytes = await readFile(path);
    checkBinary(bytes, target);
    const image = checkImage(await readFile(join(dirname(path), 'oo-lite-image'), 'utf8'));
    let doctor;
    try {
      doctor = await readFile(join(dirname(path), 'oo-mysql-doctor'));
      checkBinary(doctor, target);
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    verified.push({ target, path, bytes, image, doctor });
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
  for (const { target, bytes, image, doctor } of verified) {
    const relative = `native/${target}/oo${target.startsWith('win32-') ? '.exe' : ''}`;
    await mkdir(dirname(join(output, relative)), { recursive: true });
    await writeFile(join(output, relative), bytes);
    await chmod(join(output, relative), 0o755);
    await writeFile(join(dirname(join(output, relative)), 'oo-lite-image'), image);
    manifest.binaries[target] = { path: relative, sha256: sha256(bytes) };
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
    let output, notices, partial = false;
    const binaries = new Map();
    for (let i = 2; i < process.argv.length; i += 2) {
      const [flag, value] = process.argv.slice(i, i + 2);
      if (flag === '--output' && value && !output) output = value;
      else if (flag === '--notices' && value && !notices) notices = value;
      else if (flag === '--partial' && value === 'true') partial = true;
      else if (flag === '--binary' && value?.includes('=')) {
        const equal = value.indexOf('=');
        const target = value.slice(0, equal);
        if (binaries.has(target)) throw new Error('duplicate build target');
        binaries.set(target, value.slice(equal + 1));
      } else throw new Error('usage: stage.mjs --output /absolute/new-directory --notices /path/to/notices --binary platform-arch=/path/to/oo');
    }
    console.log(JSON.stringify(await stage({ output, binaries, notices, partial })));
  } catch (error) {
    console.error(`CLI package staging failed: ${error.message}`);
    process.exitCode = 1;
  }
}
