import { useMemo } from "react";
import { useSuperDriveSelector } from "./SuperDriveProvider";
import { dataFlowInputFromState, describeDataFlow, type DataFlow } from "./superDriveDataFlow";
import { selectDrives } from "./superDriveSelectors";

/** The data flow of the current configuration; recomputed whenever a Drive, the model or the engine health changes. */
export function useDataFlow(): DataFlow {
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d === b[i]));
  const engine = useSuperDriveSelector((state) => state.engine);
  return useMemo(() => describeDataFlow(dataFlowInputFromState(drives, engine)), [drives, engine]);
}
