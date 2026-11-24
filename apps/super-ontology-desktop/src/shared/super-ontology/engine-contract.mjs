/** Runtime enums for the Super Drive engine snapshot (20-frontend §1.5.1). One definition, shared by main and renderer. */
export const ENGINE_STATUSES = Object.freeze([
  "idle", "probing", "unsupported_platform", "oo_missing", "docker_missing", "docker_stopped", "embedded_missing",
  "stopped", "starting", "healthy", "degraded", "restarting", "crashed", "incompatible", "stopping",
]);

export const CONNECTION_STATES = Object.freeze(["connected", "reconnecting", "resyncing", "offline"]);

/**
 * Which runtime the engine uses (F-S14-01, ADR-S14): `docker` is the compose runtime, `lite` the Docker-free one (embedded PostgreSQL,
 * `oo serve --runtime host`), `auto` takes Docker when it is installed and running and the embedded runtime otherwise.
 */
export const RUNTIME_CHOICES = Object.freeze(["auto", "docker", "lite"]);
