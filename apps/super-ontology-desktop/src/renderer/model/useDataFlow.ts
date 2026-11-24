import { useMemo } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { useSuperDriveSelector } from "./SuperDriveProvider";
import { dataFlowInputFromState, describeDataFlow, type DataFlow } from "./superDriveDataFlow";
import { selectDrives } from "./superDriveSelectors";

/** The data flow of the current configuration; recomputed whenever a Drive, the model or the engine health changes. */
export function useDataFlow(): DataFlow {
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d === b[i]));
  const engine = useSuperDriveSelector((state) => state.engine);
  const { formatList } = useLocalization();
  return useMemo(() => describeDataFlow(dataFlowInputFromState(drives, engine), (items) => formatList(items, { style: "long", type: "conjunction" })), [drives, engine, formatList]);
}
