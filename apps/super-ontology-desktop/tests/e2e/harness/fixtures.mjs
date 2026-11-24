// Synthetic documents for the scenarios, created up front and chosen by full path. All names and people are invented.
import fs from "node:fs/promises";
import path from "node:path";

export const FIXTURE_FILES = Object.freeze({
  "Projects/alpha.md": "---\ntype: project\nname: Project Alpha\nowner: Dana Lee\n---\n# Project Alpha\n\nOwner: Dana Lee\n\nStatus: in progress\n",
  "People/dana-lee.md": "---\ntype: person\nname: Dana Lee\nrole: Engineering lead\n---\n# Dana Lee\n\nRole: Engineering lead\n",
  "Notes/kickoff.md": "# Kickoff notes\n\nProject Alpha starts in March. Dana Lee leads the team.\n",
});

/**
 * What the fake engine knows once the fixture folder is "processed": the files as published and the entities their front matter names.
 * The fake engine never parses anything, so a scenario that needs documents and entities on it gets them from here.
 */
export function fakeCorpus(driveId) {
  const now = new Date().toISOString();
  const pad = (n) => `00000000-0000-4000-8000-${String(n).padStart(12, "f")}`;
  const files = Object.entries(FIXTURE_FILES).map(([relative, text], index) => ({
    id: pad(index + 1), drive_id: driveId, path: relative, name: relative.split("/").pop(), size: Buffer.byteLength(text), format: "md",
    state: "published", state_since: now, retryable: false, pending_review: 0,
  }));
  const entity = (index, key, type, name, files) => ({ id: pad(100 + index), key, type, name, status: "active", counts: { facts: 2, aliases: 0 }, files, aliases: [], facts: [], revision: 1 });
  const entities = [
    entity(1, "project-alpha", "Project", "Project Alpha", [files[0].path]),
    entity(2, "dana-lee", "Person", "Dana Lee", [files[1].path]),
  ];
  return { files, entities };
}

/** The folder every scenario registers as a source ("Sample docs"), plus the same files for the managed-files drop. */
export async function createFixtures(root) {
  const folder = path.join(root, "Sample docs");
  for (const [relative, text] of Object.entries(FIXTURE_FILES)) {
    await fs.mkdir(path.dirname(path.join(folder, relative)), { recursive: true });
    await fs.writeFile(path.join(folder, relative), text, "utf8");
  }
  const loose = path.join(root, "Drop me");
  await fs.mkdir(loose, { recursive: true });
  const files = [];
  for (const relative of ["Projects/alpha.md", "People/dana-lee.md"]) {
    const target = path.join(loose, path.basename(relative));
    await fs.writeFile(target, FIXTURE_FILES[relative], "utf8");
    files.push(target);
  }
  return { folder, loose, files, count: Object.keys(FIXTURE_FILES).length };
}

/**
 * A stand-in for the embedded-runtime folder the DMG ships, so the app's preflight accepts the `lite` runtime and `oo serve` is started
 * with `--runtime embedded` (its own PostgreSQL, no Docker, no container). The engine brings the real database itself.
 */
export async function createLiteRuntimeStub(root) {
  const lite = path.join(root, "lite");
  await fs.mkdir(path.join(lite, "postgres", "bin"), { recursive: true });
  for (const binary of ["postgres", "initdb", "pg_ctl"]) await fs.writeFile(path.join(lite, "postgres", "bin", binary), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  await fs.mkdir(path.join(lite, "sidecar"), { recursive: true });
  await fs.writeFile(path.join(lite, "runtime.json"), JSON.stringify({ protocol: "oo.lite-runtime/v1", postgres: "17.4" }));
  return lite;
}

/** The synthetic md, txt and docx for the auto-sync scenario (P07): all names and people are invented. */
export function syntheticDocx(text) {
  // A minimal Word package, stored (no compression) so it needs no library: [Content_Types].xml, _rels/.rels and word/document.xml.
  const files = [
    ["[Content_Types].xml", '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>'],
    ["_rels/.rels", '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>'],
    ["word/document.xml", `<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>${text}</w:t></w:r></w:p></w:body></w:document>`],
  ];
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (buffer) => { let c = 0xffffffff; for (const byte of buffer) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, body] of files) {
    const nameBytes = Buffer.from(name);
    const data = Buffer.from(body);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(20, 4); local.writeUInt32LE(crc(data), 14); local.writeUInt32LE(data.length, 18); local.writeUInt32LE(data.length, 22); local.writeUInt16LE(nameBytes.length, 26);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0); central.writeUInt16LE(20, 4); central.writeUInt16LE(20, 6); central.writeUInt32LE(crc(data), 16); central.writeUInt32LE(data.length, 20); central.writeUInt32LE(data.length, 24); central.writeUInt16LE(nameBytes.length, 28); central.writeUInt32LE(offset, 42);
    locals.push(local, nameBytes, data);
    centrals.push(central, nameBytes);
    offset += 30 + nameBytes.length + data.length;
  }
  const centralBytes = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(files.length, 8); end.writeUInt16LE(files.length, 10); end.writeUInt32LE(centralBytes.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, centralBytes, end]);
}

export const BASE_FILE_COUNT = 20;

/**
 * Folders for P07. `watched` holds BASE_FILE_COUNT small notes (a folder of one or two files would trip the engine's mass-delete hold when a
 * file is deleted, which is the engine's rule and not what this scenario measures); `drop` is the three files dropped into My files; `staging`
 * holds the file copied through Finder and the 50 that are bulk-copied; `edits` is where the outside edits are prepared.
 */
export async function createAutosyncFixtures(root) {
  const make = async (name) => { const directory = path.join(root, name); await fs.mkdir(directory, { recursive: true }); return directory; };
  const watched = await make("Watched docs");
  for (let index = 1; index <= BASE_FILE_COUNT; index += 1) {
    const id = String(index).padStart(2, "0");
    await fs.writeFile(path.join(watched, `base-${id}.md`), `# Base note ${id}\n\nA steady note number ${id} that stays as it is.\n`, "utf8");
  }
  // Not the sample folder's files: the engine marks identical content in another source a duplicate.
  await fs.writeFile(path.join(watched, "mira-okafor.md"), "---\ntype: person\nname: Mira Okafor\nrole: Platform lead\n---\n# Mira Okafor\n\nRole: Platform lead\n", "utf8");
  await fs.writeFile(path.join(watched, "beacon.md"), "---\ntype: project\nname: Project Beacon\nowner: Mira Okafor\n---\n# Project Beacon\n\nOwner: Mira Okafor\n\nStatus: planning\n", "utf8");
  const drop = await make("Drop three");
  await fs.writeFile(path.join(drop, "note-one.md"), "# Note one\n\nA markdown note dropped into My files.\n", "utf8");
  await fs.writeFile(path.join(drop, "note-two.txt"), "A plain text note dropped into My files.\n", "utf8");
  await fs.writeFile(path.join(drop, "note-three.docx"), syntheticDocx("A Word note dropped into My files."));
  const staging = await make("Staging");
  await fs.writeFile(path.join(staging, "finder-note.md"), "---\ntype: person\nname: Finn Hale\nrole: Courier\n---\n# Finn Hale\n\nRole: Courier\n", "utf8");
  const bulk = await make("Bulk 50");
  for (let index = 1; index <= 50; index += 1) {
    const id = String(index).padStart(2, "0");
    await fs.writeFile(path.join(bulk, `bulk-${id}.md`), `# Bulk note ${id}\n\nA copied note number ${id}.\n`, "utf8");
  }
  return { watched, drop, staging, bulk };
}
