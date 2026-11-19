import type { ConnectionState, EngineStatus } from "./bridge";
export const ENGINE_STATUSES: readonly EngineStatus[];
export const CONNECTION_STATES: readonly ConnectionState[];
export type RuntimeChoice = "auto" | "docker" | "lite";
export const RUNTIME_CHOICES: readonly RuntimeChoice[];
