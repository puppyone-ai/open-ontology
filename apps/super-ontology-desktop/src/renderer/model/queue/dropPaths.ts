/** The engine accepts at most this many paths in one upload request (requests.mjs `upload-start`); a bigger drop goes in several. */
export const UPLOAD_PATHS_PER_REQUEST = 1000;

const trimmed = (value: string): string => (value.length > 1 ? value.replace(/\/+$/u, "") : value);

/**
 * The host paths of a drop, made safe to hand over: relative or empty ones go (the window cannot tell where they point), the same path twice
 * counts once, and a path inside another dropped path goes too (dropping `ProjectA/` and `ProjectA/notes.md` together copies the note once, in
 * its folder). Order is kept; `rejected` counts the paths that were not absolute.
 */
export function normalizeDropPaths(paths: readonly string[]): { paths: string[]; rejected: number } {
  const absolute = paths.map(trimmed).filter((value) => value.startsWith("/") || /^[A-Za-z]:[\\/]/u.test(value));
  const unique = [...new Set(absolute)];
  return {
    paths: unique.filter((candidate) => !unique.some((other) => other !== candidate && candidate.startsWith(`${other}${other.endsWith("/") ? "" : "/"}`))),
    rejected: paths.length - absolute.length,
  };
}
