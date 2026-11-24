import { z } from "zod";
import React from "react";
import { helper } from "./helper";

export interface QueueStats {
  depth: number;
  oldestMs: number;
}

export type QueueName = string;

export enum Health {
  Ok,
  Late,
}

export const statsSchema = z.object({ depth: z.number() });

export async function fetchStats(name: QueueName): Promise<QueueStats> {
  const response = await fetch(`/api/queues/${name}`);
  return statsSchema.parse(await response.json()) as QueueStats;
}

export const useDepth = (stats: QueueStats) => {
  return helper(stats.depth);
};

export class Poller {
  start() {
    return React.version;
  }
}
