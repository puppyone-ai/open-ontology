import { useLocalization } from "@open-ontology/design-system/localization";
import type { Tone } from "@open-ontology/design-system";
import { presentFile, type FileView } from "../../model/superDriveFileView";
import type { Tone as ModelTone } from "../../model/superDriveStateModel";
import type { FileRecord } from "../../model/superDriveState";

export const TONE: Record<ModelTone, Tone> = { neutral: "neutral", info: "accent", success: "success", warning: "warning", danger: "danger" };

/** The sentence for a file's state, resolved from the shared state matrix (the reason is a message id, never engine text). */
export function useFileStateText(): (view: FileView) => string {
  const { t } = useLocalization();
  return (view) => t(view.messageId, { ...(view.values as Record<string, string | number>), reason: view.reasonMessageId ? t(view.reasonMessageId) : "" });
}

export function fileView(file: Pick<FileRecord, "state" | "state_code" | "retryable" | "blocked_reason" | "pending_review" | "progress">): FileView {
  return presentFile(file);
}
