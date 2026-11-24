import { useMemo } from "react";
import { useSuperDriveSelector } from "../SuperDriveProvider";
import { selectActiveDrive } from "../superDriveSelectors";
import { computeBanners, writeBlockOf, type WriteBlock } from "./bannerModel";

/** Why writes are off for the active Drive (full disk, Drive being removed), or `null`. The same inputs as the banner stack, so both always agree. */
export function useWriteBlock(): WriteBlock | null {
  const engine = useSuperDriveSelector((state) => state.engine);
  const connection = useSuperDriveSelector((state) => state.connection);
  const drive = useSuperDriveSelector(selectActiveDrive);
  const hasData = useSuperDriveSelector((state) => state.drives.order.length > 0);
  return useMemo(() => writeBlockOf(computeBanners({ engine, connection, drive, hasData })), [engine, connection, drive, hasData]);
}
