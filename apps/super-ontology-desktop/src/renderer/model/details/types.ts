import type { EngineFileDetail } from "../../../shared/super-ontology/bridge";
import type { FileSummary } from "../../../shared/super-ontology/docs-api.generated";

/** The detail document with the summary fields typed (the bridge type keeps them open so a newer engine can add more). */
export type FileDetailDoc = FileSummary & Pick<EngineFileDetail, "versions" | "parse" | "llm" | "history">;
