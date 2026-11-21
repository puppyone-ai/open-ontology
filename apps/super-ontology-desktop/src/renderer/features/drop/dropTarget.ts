import type { Drive } from "../../../shared/super-ontology/docs-api.generated";

/** Where a drop would go, read from the element under the pointer. */
export type DropTarget =
  | { kind: "drive"; driveId: string | null; folder: string; name: string; managed: boolean }
  | { kind: "rejected"; name: string; reason: "cloud" | "database" };

export const MY_FILES_NAME = "My files";

/**
 * Elements that take (or refuse) a drop say so with data attributes, nothing else: `data-drop-drive` (+ optional `data-drop-folder`,
 * `data-drop-name`) accepts into that source, `data-drop-reject="cloud" | "database"` explains why not. The nearest such ancestor wins; with none,
 * a drop goes into My files.
 */
export const DROP_ATTRIBUTES = Object.freeze({ drive: "data-drop-drive", folder: "data-drop-folder", name: "data-drop-name", reject: "data-drop-reject" });

export const isMyFiles = (drive: Pick<Drive, "name" | "backend">): boolean => drive.backend === "fs" && drive.name === MY_FILES_NAME;

export function dropAttributes(drive: Pick<Drive, "id" | "name" | "backend">, folder = ""): Record<string, string> {
  if (drive.backend !== "fs") return { [DROP_ATTRIBUTES.reject]: "cloud", [DROP_ATTRIBUTES.name]: drive.name };
  return { [DROP_ATTRIBUTES.drive]: drive.id, [DROP_ATTRIBUTES.folder]: folder, [DROP_ATTRIBUTES.name]: drive.name };
}

export const databaseDropAttributes = (name: string): Record<string, string> => ({ [DROP_ATTRIBUTES.reject]: "database", [DROP_ATTRIBUTES.name]: name });

/** The element that decides the target, and what it decides. `myFiles` is the My files Drive when the engine already lists it. */
export function resolveDropTarget(element: Element | null, myFiles: Pick<Drive, "id" | "name"> | null): { target: DropTarget; element: Element | null } {
  const owner = element?.closest(`[${DROP_ATTRIBUTES.drive}], [${DROP_ATTRIBUTES.reject}]`) ?? null;
  if (owner) {
    const name = owner.getAttribute(DROP_ATTRIBUTES.name) ?? "";
    const reject = owner.getAttribute(DROP_ATTRIBUTES.reject);
    if (reject === "cloud" || reject === "database") return { target: { kind: "rejected", name, reason: reject }, element: owner };
    const driveId = owner.getAttribute(DROP_ATTRIBUTES.drive);
    if (driveId) {
      const managed = myFiles?.id === driveId;
      return { target: { kind: "drive", driveId, folder: owner.getAttribute(DROP_ATTRIBUTES.folder) ?? "", name, managed }, element: owner };
    }
  }
  return { target: { kind: "drive", driveId: myFiles?.id ?? null, folder: "", name: myFiles?.name ?? MY_FILES_NAME, managed: true }, element: null };
}

/** A drag that carries files (or folders); text, links and in-page drags do not. */
export function isFileDrag(transfer: DataTransfer | null | undefined): boolean {
  if (!transfer) return false;
  return Array.from(transfer.types ?? []).includes("Files");
}

/** How many items the drag carries, known during the drag (the names are not). At least 1 for a file drag. */
export function dragItemCount(transfer: DataTransfer | null | undefined): number {
  if (!transfer) return 0;
  const items = Array.from(transfer.items ?? []).filter((item) => item.kind === "file").length;
  return Math.max(items, transfer.files?.length ?? 0, isFileDrag(transfer) ? 1 : 0);
}
