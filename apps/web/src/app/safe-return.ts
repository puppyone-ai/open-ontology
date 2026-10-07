/** Keeps a same-origin path. Drops queries so a token cannot be echoed. */
export function safeReturnTo(value: string | null): string | null {
  if (!value || value.length > 200) {
    return null;
  }
  const path = value.split("?")[0]?.split("#")[0] ?? "";
  if (!path.startsWith("/") || path.startsWith("//") || path.includes("\\") || path.includes("://")) {
    return null;
  }
  return path;
}
