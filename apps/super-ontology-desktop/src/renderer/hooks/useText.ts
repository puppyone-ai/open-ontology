import { useLocalization } from "@open-ontology/design-system/localization";
import { resolveErrorMessageId, type SuperDriveError } from "../model/superDriveErrors";
import { hasSuperDriveMessage } from "../model/superDriveMessageKeys";

/** The user-facing sentence for an engine or bridge error: what happened, from the shared error catalog. */
export function useErrorText(): (error: SuperDriveError) => string {
  const { t } = useLocalization();
  return (error) => t(resolveErrorMessageId(error.code, hasSuperDriveMessage), { code: error.code, ...(error.details as Record<string, string | number>) });
}

/** "3 minutes ago" for a date, in the active language. */
export function useRelativeTime(): (value: Date | string | number) => string {
  const { formatRelativeTime } = useLocalization();
  return (value) => {
    const seconds = Math.round((new Date(value).getTime() - Date.now()) / 1000);
    const abs = Math.abs(seconds);
    if (abs < 45) return formatRelativeTime(0, "second", { numeric: "auto" });
    if (abs < 3600) return formatRelativeTime(Math.round(seconds / 60), "minute", { numeric: "auto" });
    if (abs < 86_400) return formatRelativeTime(Math.round(seconds / 3600), "hour", { numeric: "auto" });
    if (abs < 2_592_000) return formatRelativeTime(Math.round(seconds / 86_400), "day", { numeric: "auto" });
    return formatRelativeTime(Math.round(seconds / 2_592_000), "month", { numeric: "auto" });
  };
}
