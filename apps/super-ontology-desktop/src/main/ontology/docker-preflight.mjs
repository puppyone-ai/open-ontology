import fs from "node:fs/promises";
import path from "node:path";
import { runBoundedProcessProbe } from "./bounded-process-probe.mjs";
import { PROBE_TIMEOUT_MS } from "./engine-locator.mjs";

async function probeJson(runProbe, executable, args, { environment, timeoutMs }) {
  try {
    const result = await runProbe(executable, args, {
      env: environment, timeoutMs, maxOutputBytes: 256 * 1024, label: "docker probe",
    });
    let value = null;
    try { value = JSON.parse(result.stdout); } catch { /* Not JSON: treated as unusable. */ }
    return { code: result.code, value: value && typeof value === "object" ? value : null };
  } catch {
    // Timeout, spawn failure or oversized output: the daemon is not usable.
    return { code: null, value: null };
  }
}

/**
 * Docker presence and daemon state, each probe bounded (20-frontend §1.2.3).
 * `running` needs `docker info` to succeed; the version comes from the client.
 */
export async function probeDocker({
  dockerPath,
  environment,
  runProbe = runBoundedProcessProbe,
  timeoutMs = PROBE_TIMEOUT_MS,
}) {
  if (!dockerPath) return { installed: false, running: false, version: null };
  const options = { environment, timeoutMs };
  const [versionResult, infoResult] = await Promise.all([
    probeJson(runProbe, dockerPath, ["version", "--format", "{{json .}}"], options),
    probeJson(runProbe, dockerPath, ["info", "--format", "{{json .}}"], options),
  ]);
  const clientVersion = versionResult.value?.Client?.Version;
  const serverVersion = infoResult.value?.ServerVersion ?? versionResult.value?.Server?.Version;
  const version = [clientVersion, serverVersion].find((value) => typeof value === "string" && value) ?? null;
  return { installed: true, running: infoResult.code === 0 && infoResult.value !== null, version };
}

/** Free bytes on the volume holding `directory`; `null` when it cannot be read. */
export async function probeDiskFreeBytes(directory, { statfs = fs.statfs, fsPromises = fs } = {}) {
  try {
    // The state directory may not exist yet: measure its nearest existing parent.
    let target = directory;
    for (let depth = 0; depth < 32; depth += 1) {
      try { await fsPromises.access(target); break; } catch { target = path.dirname(target); }
    }
    const stats = await statfs(target);
    const free = Number(stats.bavail) * Number(stats.bsize);
    return Number.isFinite(free) && free >= 0 ? free : null;
  } catch {
    return null;
  }
}

/**
 * One full preflight pass (20-frontend §1.4.1 "probing"): platform, `oo`,
 * Docker, free disk. Returns the renderer-visible `preflight` plus the
 * resulting engine status and code. Never starts anything.
 */
export function createEnginePreflight({
  platform,
  locator,
  runProbe = runBoundedProcessProbe,
  statfs = fs.statfs,
  timeoutMs = PROBE_TIMEOUT_MS,
  liteRuntime = { probe: async () => ({ found: false, postgres: { found: false, version: null }, sidecar: { found: false } }) },
  getRuntimeChoice = () => "auto",
}) {
  return Object.freeze({
    async run({ userExecutable = null, stateDir, signal } = {}) {
      if (platform === "windows") {
        return classify({
          platform: "unsupported",
          oo: { found: false, version: null, source: null },
          docker: { installed: false, running: false, version: null },
          diskFreeBytes: null,
        }, false);
      }
      const choice = getRuntimeChoice();
      const located = await locator.locate({ userExecutable, signal });
      // `lite` never looks at Docker; `docker` never looks at the embedded runtime; `auto` takes Docker when it works and the
      // embedded runtime when Docker does not (the Mac without Docker: no terminal, nothing to install first).
      let docker = { installed: false, running: false, version: null };
      let embedded = null;
      if (choice !== "lite") docker = await probeDocker({ dockerPath: located.dockerPath, environment: located.environment, runProbe, timeoutMs });
      if (choice !== "docker") embedded = await liteRuntime.probe();
      const runtime = choice === "lite" || (choice === "auto" && !(docker.installed && docker.running) && embedded?.found === true) ? "lite" : "docker";
      const diskFreeBytes = stateDir ? await probeDiskFreeBytes(stateDir, { statfs }) : null;
      return {
        ...classify({
          platform: "ok",
          oo: located.oo
            ? { found: true, version: located.oo.version, source: located.oo.source }
            : { found: false, version: null, source: null },
          docker,
          diskFreeBytes,
          runtime,
          runtimeChoice: choice,
          embedded: runtime === "lite" ? embedded : null,
        }, located.ooInvalid),
        runtime,
        ooPath: located.oo?.path ?? null,
        dockerPath: located.dockerPath,
        environment: located.environment,
      };
    },
  });
}

/**
 * Order of the checks is the order of 20-frontend §4.4: platform, oo, docker installed, docker running. With the Docker-free runtime
 * (F-S14-01) the Docker checks drop out and the embedded database takes their place.
 */
function classify(preflight, ooInvalid) {
  if (preflight.platform !== "ok") return { preflight, status: "unsupported_platform", code: "SO_PLATFORM_UNSUPPORTED" };
  if (!preflight.oo.found) {
    return { preflight, status: "oo_missing", code: ooInvalid ? "SO_OO_INVALID" : "SO_OO_MISSING" };
  }
  // The Docker-free runtime has no Docker rows: it needs the embedded database, and nothing else of Docker.
  if (preflight.runtime === "lite") {
    return preflight.embedded?.found === true
      ? { preflight, status: "stopped", code: null }
      : { preflight, status: "embedded_missing", code: "SO_EMBEDDED_RUNTIME_MISSING" };
  }
  if (!preflight.docker.installed) return { preflight, status: "docker_missing", code: "SO_DOCKER_MISSING" };
  if (!preflight.docker.running) return { preflight, status: "docker_stopped", code: "SO_DOCKER_STOPPED" };
  return { preflight, status: "stopped", code: null };
}
