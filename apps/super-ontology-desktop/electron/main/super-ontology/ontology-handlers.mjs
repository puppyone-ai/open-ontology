import { randomUUID } from "node:crypto";
import path from "node:path";
import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";
import { handlerResult } from "./handler-result.mjs";
import { ensure, isItemPage, isPage } from "./response-guards.mjs";

/** One decide request carries at most this many items (contract ReviewDecideRequest). */
export const DECIDE_BATCH_SIZE = 500;

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const isFileSummaryLike = (value) => isObject(value) && typeof value.id === "string" && typeof value.path === "string" && typeof value.drive_id === "string";
const isShare = (value) => isObject(value) && typeof value.id === "string" && typeof value.state === "string";
const isEntity = (value) => isObject(value) && typeof value.id === "string" && typeof value.name === "string";
const isFact = (value) => isObject(value) && typeof value.id === "string" && typeof value.predicate === "string";
const isPack = (value) => isObject(value) && typeof value.id === "string" && typeof value.version === "number";
const isModelConfig = (value) => isObject(value) && typeof value.configured === "boolean";
const isUsage = (value) => isObject(value);
const isDecideResult = (value) => isObject(value);
const isRevoke = (value) => isObject(value);
const isBatch = (value) => isObject(value) && Array.isArray(value.results);
const isResolve = (value) => isObject(value);

/** Maps the renderer's camelCase compile request to the contract's `CompileRequest`. */
function compileBody(compile) {
  return {
    drive_id: compile.driveId,
    ...(compile.scope ? { scope: compile.scope } : {}),
    ...(compile.task === undefined ? {} : { task: compile.task }),
    ...(compile.seeds ? { seeds: compile.seeds } : {}),
    ...(compile.budget === undefined ? {} : { budget: compile.budget }),
    ...(compile.role ? { role: compile.role } : {}),
    ...(compile.version === undefined ? {} : { version: compile.version }),
    ...(compile.requireComplete === undefined ? {} : { require_complete: compile.requireComplete }),
    ...(compile.waitMs === undefined ? {} : { wait_ms: compile.waitMs }),
    ...(compile.minDriveRev === undefined ? {} : { min_drive_rev: compile.minDriveRev }),
    ...(compile.vector ? { vector: compile.vector } : {}),
  };
}

/**
 * IPC handlers of the L4 and L6 channels: reviews, entities, packs, shares, usage and the model. Each turns a validated
 * request into control API calls; `share-mount` and `model-config-save` are the only places a host path or a secret is
 * handled, and both resolve them here, never in the renderer.
 */
export function createOntologyHandlers({ client, grants, revealPath = () => {}, secretsDirectory = null, fsPromises, mediaDecoder = null, hostMount = null }) {
  const call = (name, options) => client.request(name, options);
  const decoding = new Map();

  /** The engine reports `mount.present: false` and never learns a host path: the host fills `mount` from its own record. */
  const withMounts = async (shares) => (hostMount ? hostMount.annotate(shares) : shares);

  async function readShare(shareId) {
    const { value } = await call("shareGet", { params: { sid: shareId } });
    ensure(value, isShare);
    return (await withMounts([value]))[0];
  }

  return {
    "entities-search": async ({ q, driveId, type, limit, cursor }) => {
      const { value } = await call("entitiesList", { query: { q: q || undefined, drive_id: driveId, type, limit, cursor } });
      return ensure(value, isPage);
    },

    "entity-detail": async ({ entityIdOrKey, includeHistory }) => {
      const { value } = await call("entityGet", { params: { eid: entityIdOrKey }, query: { include: includeHistory ? "history" : undefined } });
      return ensure(value, isEntity);
    },

    "resolve": async ({ text, driveId, types, limit }) => {
      const body = { text, ...(driveId ? { drive_id: driveId } : {}), ...(types ? { types } : {}), ...(limit ? { limit } : {}) };
      const { value } = await call("resolve", { body });
      return ensure(value, isResolve);
    },

    "review-list": async ({ driveId, kind, source, status, groupBy, limit, cursor }) => {
      const { value } = await call("reviewList", {
        query: { drive_id: driveId, kind, source, status: status ?? "pending", group_by: groupBy, limit, cursor },
      });
      return ensure(value, isPage);
    },

    /**
     * One decision request. `items` are cut into batches of 500, each with its own idempotency key derived from the
     * operation id, and the per-item results are merged; `bySource` is one request.
     */
    "review-decide": async ({ operationId, items, bySource }) => {
      if (bySource) {
        const body = {
          by_source: {
            file_id: bySource.fileId, decision: bySource.decision,
            ...(bySource.kinds ? { kinds: bySource.kinds } : {}),
            ...(bySource.asOfSeq === undefined ? {} : { as_of_seq: bySource.asOfSeq }),
          },
        };
        const { value, replayed } = await call("reviewDecide", { body, operationId });
        return handlerResult(ensure(value, isDecideResult), { replayed });
      }
      if (!items) throw new SuperOntologyError("SO_INVALID_REQUEST");
      const merged = { results: [], accepted: 0, rejected: 0, publish_pending: false };
      let replayedAll = true;
      for (let start = 0, batch = 0; start < items.length; start += DECIDE_BATCH_SIZE, batch += 1) {
        const body = { items: items.slice(start, start + DECIDE_BATCH_SIZE).map((item) => ({ ...item })) };
        const { value, replayed } = await call("reviewDecide", { body, operationId: items.length > DECIDE_BATCH_SIZE ? `${operationId}-${batch}` : operationId });
        ensure(value, isDecideResult);
        replayedAll &&= replayed;
        merged.results.push(...(value.results ?? []));
        merged.accepted += Number(value.accepted) || 0;
        merged.rejected += Number(value.rejected) || 0;
        merged.publish_pending ||= value.publish_pending === true;
      }
      return handlerResult(merged, { replayed: replayedAll });
    },

    "pack-compile": async ({ operationId, compile }) => {
      const { value, replayed } = await call("packCompile", { body: compileBody(compile), operationId });
      return handlerResult(ensure(value, isPack), { replayed });
    },

    "pack-read": async ({ packId }) => {
      const { value } = await call("packGet", { params: { pid: packId } });
      return ensure(value, isPack);
    },

    "shares-list": async ({ driveId, states, limit, cursor }) => {
      const { value } = await call("sharesList", { query: { drive_id: driveId, state: states, limit, cursor } });
      ensure(value, isItemPage);
      for (const item of value.items) ensure(item, isShare);
      return { ...value, items: await withMounts(value.items) };
    },

    "share-get": async ({ shareId }) => readShare(shareId),

    /** The mount target is a grant from the native picker; the engine records the Share, the host writes the folder (host-mount.mjs). */
    "share-mount": async ({ operationId, compile, targetGrantId, expiresInSeconds, group, refreshPolicy }, { sender } = {}) => {
      const picked = grants.resolve(sender, targetGrantId, "mount-target", operationId);
      const targetDir = hostMount ? await hostMount.preflight(picked) : picked;
      const body = {
        compile: compileBody(compile),
        mount: { target_dir: targetDir },
        expires_in_seconds: expiresInSeconds,
        ...(group ? { group } : {}),
        ...(refreshPolicy ? { refresh_policy: refreshPolicy } : {}),
      };
      const { value, replayed } = await call("sharesCreate", { body, operationId });
      ensure(value, isShare);
      if (hostMount && !replayed) {
        try {
          await hostMount.install(value, targetDir);
        } catch (error) {
          // Nothing was mounted: do not leave a live Share behind.
          await call("shareRevoke", { params: { sid: value.id }, body: {}, operationId: randomUUID() }).catch(() => {});
          throw error;
        }
      }
      return handlerResult((await withMounts([value]))[0], { replayed });
    },

    "share-refresh": async ({ operationId, shareId }) => {
      const { value, replayed } = await call("shareRefresh", { params: { sid: shareId }, body: {}, operationId });
      ensure(value, isShare);
      if (hostMount && !replayed) await hostMount.replace(shareId, value);
      return handlerResult((await withMounts([value]))[0], { replayed });
    },

    "share-revoke": async ({ operationId, shareId, reason }) => {
      const { value, replayed } = await call("shareRevoke", { params: { sid: shareId }, body: reason ? { reason } : {}, operationId });
      ensure(value, isRevoke);
      if (hostMount) await hostMount.remove(shareId).catch(() => {});
      return handlerResult(value, { replayed });
    },

    /** Shows the mounted folder in the file manager; only the folder the engine recorded for this Share. */
    "share-reveal": async ({ shareId }) => {
      const share = await readShare(shareId);
      const target = share.mount?.target_dir;
      if (typeof target !== "string" || !path.isAbsolute(target) || share.mount?.present === false) return { revealed: false };
      revealPath(target);
      return { revealed: true };
    },

    "share-updates": async ({ shareId, limit, cursor }) => {
      const { value } = await call("shareUpdates", { params: { sid: shareId }, query: { limit, cursor } });
      return ensure(value, isItemPage);
    },

    "share-group-refresh": async ({ operationId, groupId }) => {
      const { value, replayed } = await call("shareGroupRefresh", { params: { gid: groupId }, body: {}, operationId });
      ensure(value, isBatch);
      if (hostMount && !replayed && Array.isArray(value.refreshed)) {
        for (const [index, fresh] of value.refreshed.entries()) {
          const old = value.results[index]?.id;
          if (typeof old === "string" && isShare(fresh)) await hostMount.replace(old, fresh);
        }
      }
      return handlerResult(value, { replayed });
    },

    "usage-read": async ({ driveId, days }) => {
      const { value } = await call("usage", { query: { drive_id: driveId, days } });
      return ensure(value, isUsage);
    },

    "atlas-links": async () => {
      const { value } = await call("linksList", {});
      return ensure(value, (v) => isObject(v) && Array.isArray(v.items));
    },

    "atlas-sql": async ({ sql, maxRows }) => {
      const { value } = await call("ontologyQuery", { body: { sql, ...(maxRows ? { max_rows: maxRows } : {}) } });
      return ensure(value, (v) => isObject(v) && Array.isArray(v.columns) && Array.isArray(v.rows));
    },

    "ontology-versions": async ({ limit, cursor }) => {
      const { value } = await call("ontologyVersions", { query: { limit, cursor } });
      return ensure(value, isPage);
    },

    "model-config-read": async () => {
      const { value } = await call("modelConfigGet", {});
      return ensure(value, isModelConfig);
    },

    /**
     * The key is written to a 0600 file inside the secrets directory next to the engine state and only a `file:` reference reaches the
     * engine (20 section 8.2 Q5); without a new key the reference the engine already holds is kept.
     */
    "model-config-save": async ({ provider, baseUrl, model, apiKey, revision = 0 }) => {
      let keyRef;
      if (apiKey) {
        if (!secretsDirectory) throw new SuperOntologyError("SO_SECRET_STORE_UNAVAILABLE");
        const directory = secretsDirectory;
        const file = path.join(directory, "model-key");
        try {
          await fsPromises.mkdir(directory, { recursive: true, mode: 0o700 });
          await fsPromises.writeFile(file, apiKey, { mode: 0o600 });
          await fsPromises.chmod(file, 0o600);
        } catch {
          throw new SuperOntologyError("SO_SECRET_STORE_UNAVAILABLE");
        }
        keyRef = `file:${file}`;
      } else {
        const { value: current } = await call("modelConfigGet", {});
        ensure(current, isModelConfig);
        if (typeof current.key_ref !== "string") throw new SuperOntologyError("SO_INVALID_REQUEST");
        keyRef = current.key_ref;
      }
      const { value } = await call("modelConfigPut", { body: { provider, base_url: baseUrl, model, key_ref: keyRef }, revision });
      return ensure(value, isModelConfig);
    },

    "fact-correct": async ({ operationId, factId, revision, action, value: newValue, reason }) => {
      const body = { action, ...(newValue === undefined ? {} : { value: newValue }), ...(reason ? { reason } : {}) };
      const { value, replayed } = await call("factCorrect", { params: { id: factId }, body, operationId, revision });
      return handlerResult(ensure(value, isFact), { replayed });
    },

    "entity-merge": async ({ operationId, into, from, revision }) => {
      const { value, replayed } = await call("entitiesMerge", { body: { into, from }, operationId, revision });
      return handlerResult(ensure(value, isEntity), { replayed });
    },

    "entity-split": async ({ operationId, entityId, revision, groups }) => {
      const { value, replayed } = await call("entitySplit", { params: { eid: entityId }, body: { groups }, operationId, revision });
      return handlerResult(ensure(value, isBatch), { replayed });
    },

    /**
     * Decodes a media file on this computer (macOS only) into the engine's exchange directory; the engine then parses it. The
     * source is the file of an fs Drive, resolved here from the Drive root and never from a renderer path.
     */
    "media-decode": async ({ fileId }) => {
      if (!mediaDecoder || !(await mediaDecoder.ready())) throw new SuperOntologyError("SO_PLATFORM_UNSUPPORTED");
      const { value: file } = await call("fileDetail", { params: { fid: fileId }, query: {} });
      ensure(file, isFileSummaryLike);
      const { value: drive } = await call("driveGet", { params: { id: file.drive_id } });
      const root = drive?.config?.root;
      if (drive?.backend !== "fs" || typeof root !== "string" || !path.isAbsolute(root)) throw new SuperOntologyError("SO_UPLOAD_UNSUPPORTED_BACKEND");
      const target = path.resolve(root, file.path);
      if (target !== path.resolve(root) && !target.startsWith(`${path.resolve(root)}${path.sep}`)) throw new SuperOntologyError("SO_INVALID_REQUEST");
      let real;
      try { real = await fsPromises.realpath(target); } catch { throw new SuperOntologyError("SO_UPLOAD_SOURCE_MISSING"); }
      const realRoot = await fsPromises.realpath(root).catch(() => path.resolve(root));
      if (!real.startsWith(`${realRoot}${path.sep}`)) throw new SuperOntologyError("SO_INVALID_REQUEST");
      decoding.get(fileId)?.abort();
      const controller = new AbortController();
      decoding.set(fileId, controller);
      try {
        const result = await mediaDecoder.decode({ sourcePath: real, signal: controller.signal });
        return { decoded: true, durationMs: result.durationMs, frames: result.frames, hasAudio: result.hasAudio, still: result.still, helperVersion: result.helperVersion };
      } finally {
        if (decoding.get(fileId) === controller) decoding.delete(fileId);
      }
    },

    "media-decode-cancel": async ({ fileId }) => {
      const controller = decoding.get(fileId);
      controller?.abort();
      return { cancelled: controller !== undefined };
    },

    "constraints-list": async ({ limit, cursor }) => {
      const { value } = await call("constraintsList", { query: { limit, cursor } });
      return ensure(value, isPage);
    },
  };
}
