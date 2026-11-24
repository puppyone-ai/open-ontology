/**
 * Small formatting helpers kept inside the feature so Super Drive does not
 * reach into `cloud/utils` (20-frontend §7.2 F-S2-05). Text goes through the
 * localization layer; nothing here concatenates untranslated words.
 */
import type { LocaleFormatters, MessageFormatter } from "@open-ontology/design-system/localization/core";

export type FormatContext = {
  t: MessageFormatter;
  formatNumber: LocaleFormatters["formatNumber"];
  formatDate: LocaleFormatters["formatDate"];
  formatRelativeTime: LocaleFormatters["formatRelativeTime"];
};

export function shellQuote(value: string): string {
  return `'${value.replace(/'/gu, "'\\''")}'`;
}

export async function copyText(value: string): Promise<void> {
  // The Electron shell writes the clipboard for the sandboxed window; a plain browser (development bridge) uses the web API.
  if (window.superOntology?.shell?.copyToClipboard) { await window.superOntology.shell.copyToClipboard(value); return; }
  await navigator.clipboard.writeText(value);
}

const KIB = 1024;
const MIB = KIB * 1024;
const GIB = MIB * 1024;

/** Locale-aware size using the units of the `super-drive.size.*` messages. */
export function formatBytes(bytes: number | null | undefined, context: Pick<FormatContext, "t" | "formatNumber">): string {
  if (typeof bytes !== "number" || !Number.isFinite(bytes) || bytes < 0) return "";
  const fraction = { maximumFractionDigits: 1 };
  if (bytes >= GIB) return context.t("super-drive.size.gigabytes", { value: context.formatNumber(bytes / GIB, fraction) });
  if (bytes >= MIB) return context.t("super-drive.size.megabytes", { value: context.formatNumber(bytes / MIB, fraction) });
  if (bytes >= KIB) return context.t("super-drive.size.kilobytes", { value: context.formatNumber(bytes / KIB, fraction) });
  return context.t("super-drive.size.bytes", { value: context.formatNumber(bytes) });
}

/** "just now", "3 minutes ago", then a short date once it is older than a week. */
export function formatRelativeTime(
  iso: string | null | undefined,
  context: Pick<FormatContext, "formatDate" | "formatRelativeTime">,
  now: number = Date.now(),
): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const deltaMs = date.getTime() - now;
  const absolute = Math.abs(deltaMs);
  if (absolute < 60_000) return context.formatRelativeTime(0, "second", { numeric: "auto" });
  if (absolute < 3_600_000) return context.formatRelativeTime(Math.round(deltaMs / 60_000), "minute", { numeric: "auto" });
  if (absolute < 86_400_000) return context.formatRelativeTime(Math.round(deltaMs / 3_600_000), "hour", { numeric: "auto" });
  if (absolute < 604_800_000) return context.formatRelativeTime(Math.round(deltaMs / 86_400_000), "day", { numeric: "auto" });
  return context.formatDate(date, {
    month: "short",
    day: "numeric",
    year: date.getFullYear() !== new Date(now).getFullYear() ? "numeric" : undefined,
  });
}
