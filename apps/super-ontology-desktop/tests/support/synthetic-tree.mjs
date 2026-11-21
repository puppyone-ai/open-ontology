// A synthetic code-project tree for the drop and scale tests: many kept source files plus the usual litter a code project carries
// (.git, node_modules, __pycache__, .venv, target, dist, *.pyc), a .gitignore and an .ooignore. All content is generated.
import fs from "node:fs/promises";
import path from "node:path";

const EXTENSIONS = ["ts", "py", "rs", "md", "json", "txt", "go", "css"];
const BINARY_ASSETS = 60;

/** `text` is a string (every file the same) or a function of the file name (unique content: the engine keeps a file per content, so a test that wants many parsed files needs it). */
async function writeMany(directory, names, text) {
  await fs.mkdir(directory, { recursive: true });
  for (let from = 0; from < names.length; from += 200) {
    await Promise.all(names.slice(from, from + 200).map((name) => fs.writeFile(path.join(directory, name), typeof text === "function" ? text(name) : text)));
  }
}

/**
 * @param {string} root directory to create (must not exist yet)
 * @param {{ kept?: number, litter?: number }} options kept = files the rules keep; litter = files in each ignored folder
 * @returns {Promise<{ root: string, kept: number, ignoredFiles: number, skipped: { hidden: number, junk: number, gitignore: number, ooignore: number } }>}
 *   `kept` = files the rules keep; `skipped` = entries the walk counts by reason (a skipped folder counts once); `ignoredFiles` = files inside them.
 */
export async function createSyntheticTree(root, { kept = 25_000, litter = 3_000 } = {}) {
  await fs.mkdir(root, { recursive: true });
  const perDirectory = 50;
  const directories = Math.ceil(kept / perDirectory);
  let written = 0;
  for (let index = 0; index < directories; index += 1) {
    const count = Math.min(perDirectory, kept - written);
    const names = Array.from({ length: count }, (_, file) => `file-${file}.${EXTENSIONS[(index + file) % EXTENSIONS.length]}`);
    await writeMany(path.join(root, "src", `pkg-${Math.floor(index / 20)}`, `mod-${index}`), names, (name) => `// synthetic source file pkg-${Math.floor(index / 20)}/mod-${index}/${name}\n`);
    written += count;
  }
  const litterNames = Array.from({ length: litter }, (_, index) => `x-${index}.js`);
  for (const folder of ["node_modules/dep", ".git/objects", "__pycache__", ".venv/lib", "target/debug", "dist", "build", ".next/cache"]) await writeMany(path.join(root, folder), litterNames, "x");
  await writeMany(path.join(root, "src"), ["a.pyc", "b.pyc"], "x");
  // Binary assets the engine cannot read (unsupported), kept by every rule: the Not connected list must show them as one entry.
  await writeMany(path.join(root, "assets"), Array.from({ length: BINARY_ASSETS }, (_, index) => `blob-${index}.bin`), (name) => Buffer.from([0, 1, 2, 3, 0, 255, ...Buffer.from(name)]));
  await writeMany(path.join(root, "generated"), ["g-1.ts", "g-2.ts", "g-3.ts"], "// ignored by .gitignore\n");
  await writeMany(path.join(root, "logs"), ["run-1.log", "run-2.log"], "log\n");
  await writeMany(path.join(root, "secrets"), ["key-1.txt", "key-2.txt"], "ignored by .ooignore\n");
  await fs.writeFile(path.join(root, ".gitignore"), "# build output\ngenerated/\n*.log\n");
  await fs.writeFile(path.join(root, ".ooignore"), "secrets/\n");
  await fs.writeFile(path.join(root, "README.md"), "# Synthetic project\n");
  return { root, kept: kept + 1 + BINARY_ASSETS, ignoredFiles: litter * 8 + 11, skipped: { hidden: 5, junk: 7, gitignore: 3, ooignore: 1 } };
}
