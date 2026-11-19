// Synthetic documents for the scenarios, created up front and chosen by full path. All names and people are invented.
import fs from "node:fs/promises";
import path from "node:path";

export const FIXTURE_FILES = Object.freeze({
  "Projects/alpha.md": "---\ntype: project\nname: Project Alpha\nowner: Dana Lee\n---\n# Project Alpha\n\nOwner: Dana Lee\n\nStatus: in progress\n",
  "People/dana-lee.md": "---\ntype: person\nname: Dana Lee\nrole: Engineering lead\n---\n# Dana Lee\n\nRole: Engineering lead\n",
  "Notes/kickoff.md": "# Kickoff notes\n\nProject Alpha starts in March. Dana Lee leads the team.\n",
});

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
