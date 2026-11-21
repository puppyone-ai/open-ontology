import fs from "node:fs/promises";
import { SuperOntologyError, pickDetails } from "../../../src/shared/super-ontology/errors.mjs";
import { handlerResult } from "./handler-result.mjs";
import { ensure, isBatchResult, isDrive, isFileSummary, isPage, isSyncInfo, isTree } from "./response-guards.mjs";

/** Name of the default managed Drive; the one source a person never has to configure. */
export const MY_FILES_NAME = "My files";
const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const DRIVE_PAGE_LIMIT = 200;
const LOOPBACK_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"]);

/** `https://` anywhere, or `http://` only to this computer (a local object store such as RustFS). */
export function isAllowedEndpoint(value) {
  let url;
  try { url = new URL(value); } catch { return false; }
  if (url.username !== "" || url.password !== "") return false;
  return url.protocol === "https:" || (url.protocol === "http:" && LOOPBACK_HOSTS.has(url.hostname));
}
const DRIVE_PAGE_CAP = 50;
/** One retry request carries at most this many ids (contract RetryRequest). */
export const RETRY_BATCH_SIZE = 500;
const MATCHING_PAGE_LIMIT = 500;
/** Upper bound of ids a "retry everything that matches" walks: 50 pages of 500. */
const MATCHING_PAGE_CAP = 50;

/** The S3 compatible backends (docs-api 1.4); anything else is a folder Drive. */
const OBJECT_STORE_BACKENDS = new Set(["s3", "oss", "cos", "r2"]);

/** `config.policy` of a Drive (docs-api 1.4): only what the request carries; an empty list is left out. */
function policyOf({ allowPrefixes, denyPrefixes }) {
  const policy = {
    ...(allowPrefixes?.length ? { allow: allowPrefixes } : {}),
    ...(denyPrefixes?.length ? { deny: denyPrefixes } : {}),
  };
  return Object.keys(policy).length > 0 ? { policy } : {};
}

/**
 * IPC handlers that call the control API (10 §8.3). Each handler turns a
 * validated request into one or more `client.request` calls and returns the
 * renderer-visible value; grants are resolved to host paths here and nowhere
 * else.
 */
export function createControlHandlers({ client, grants, lifecycle, secretFiles, managedFilesDir = null, fsPromises = fs }) {
  const call = (name, options) => client.request(name, options);

  async function readDrive(driveId) {
    const { value } = await call("driveGet", { params: { id: driveId } });
    return ensure(value, isDrive);
  }

  /**
   * Registers an object-store Drive (s3, oss, cos or r2, all the S3 compatible path): the keys become private files and only
   * their `file:` references go to the engine (§1.2.3). A definite refusal by the engine removes the files again; an uncertain
   * outcome keeps them, because the same operation id writes the same files and the engine replays the created Drive.
   * `s3` needs an endpoint; the three clouds derive it from `region` (oss, cos) or `account_id` (r2) and take `endpoint` only
   * as an override. Whatever else is wrong (a region the cloud does not have, an account id that is not 32 hex) the engine says.
   */
  async function createObjectStoreDrive({ request, options }) {
    const { operationId, name, backend, endpoint, region, bucket, prefix, pathStyle, accountId, internal, accessKeyId, secretAccessKey, sessionToken, exclude } = request;
    if (endpoint !== undefined ? (typeof endpoint !== "string" || !isAllowedEndpoint(endpoint)) : backend === "s3") throw new SuperOntologyError("DOCS_SOURCE_ENDPOINT_INVALID");
    if (typeof bucket !== "string") throw new SuperOntologyError("DOCS_SOURCE_BUCKET_NOT_FOUND");
    if (typeof accessKeyId !== "string" || typeof secretAccessKey !== "string") throw new SuperOntologyError("DOCS_SOURCE_CREDENTIAL_MISSING");
    if (!secretFiles) throw new SuperOntologyError("SO_SECRET_STORE_UNAVAILABLE");
    const refs = await secretFiles.writeSet(operationId, { access_key_id: accessKeyId, secret_access_key: secretAccessKey, session_token: sessionToken });
    const body = {
      name, backend,
      config: {
        bucket, ...refs,
        ...(endpoint ? { endpoint } : {}),
        ...(region ? { region } : {}), ...(prefix ? { prefix } : {}), ...(pathStyle === undefined ? {} : { path_style: pathStyle }),
        ...(accountId ? { account_id: accountId } : {}), ...(internal === undefined ? {} : { internal }),
        ...(exclude ? { exclude } : {}),
        ...policyOf(request),
      },
      ...options,
    };
    try {
      const { value, replayed } = await call("driveCreate", { body, operationId });
      ensure(value, isDrive);
      return handlerResult(value, { replayed });
    } catch (error) {
      if (typeof error?.code === "string" && error.code.startsWith("DOCS_")) await secretFiles.removeSet(operationId);
      throw error;
    }
  }

  return {
    /** Walks every page so the renderer sees the complete Drive list. */
    "drives-list": async () => {
      const drives = [];
      let cursor;
      for (let page = 0; page < DRIVE_PAGE_CAP; page += 1) {
        const { value } = await call("drivesList", { query: { limit: DRIVE_PAGE_LIMIT, cursor } });
        ensure(value, isPage);
        for (const item of value.items) drives.push(ensure(item, isDrive));
        if (!value.next_cursor) return drives;
        cursor = value.next_cursor;
      }
      throw new SuperOntologyError("SO_RESPONSE_INVALID");
    },

    "drive-get": async ({ driveId }) => readDrive(driveId),

    /** The default managed Drive: a folder in the app's data directory. Idempotent; found again by its name and backend. */
    "my-files-ensure": async ({ operationId }) => {
      if (!managedFilesDir) throw new SuperOntologyError("SO_INVALID_REQUEST");
      await fsPromises.mkdir(managedFilesDir, { recursive: true, mode: 0o700 });
      const { value: listed } = await call("drivesList", { query: { limit: 200 } });
      ensure(listed, isPage);
      const existing = listed.items.find((item) => isObject(item) && item.name === MY_FILES_NAME && item.backend === "fs" && item.status !== "deleting" && item.status !== "deleted");
      if (existing) return ensure(existing, isDrive);
      const { value, replayed } = await call("driveCreate", { body: { name: MY_FILES_NAME, backend: "fs", config: { root: managedFilesDir }, model_enabled: true }, operationId });
      ensure(value, isDrive);
      return handlerResult(value, { replayed });
    },

    "drive-create": async (request, { sender } = {}) => {
      const { operationId, name, backend, exclude, modelEnabled, llmDailyCalls } = request;
      // A folder on this computer allows the model unless the request says otherwise: the model is only ever asked once the person
      // connected one in Settings, and the Privacy tab shows where excerpts go. A cloud bucket keeps the engine's default (off).
      const options = {
        ...(modelEnabled === undefined ? (OBJECT_STORE_BACKENDS.has(backend) ? {} : { model_enabled: true }) : { model_enabled: modelEnabled }),
        ...(llmDailyCalls === undefined ? {} : { llm_daily_calls: llmDailyCalls }),
      };
      if (OBJECT_STORE_BACKENDS.has(backend)) return createObjectStoreDrive({ request, options });
      if (typeof request.rootGrantId !== "string") throw new SuperOntologyError("SO_INVALID_REQUEST");
      const root = grants.resolve(sender, request.rootGrantId, "drive-root", operationId);
      const body = { name, backend: "fs", config: { root, ...(exclude ? { exclude } : {}), ...policyOf(request) }, ...options };
      const { value, replayed } = await call("driveCreate", { body, operationId });
      ensure(value, isDrive);
      if (!replayed) lifecycle.beginDriveChange();
      return handlerResult(value, { replayed });
    },

    "drive-update": async ({ driveId, revision, patch }) => {
      const { value } = await call("driveUpdate", { params: { id: driveId }, body: patch, revision });
      return ensure(value, isDrive);
    },

    "drive-delete": async ({ driveId, revision }) => {
      const { value } = await call("driveDelete", { params: { id: driveId }, query: { purge: "false" }, revision });
      ensure(value, isDrive);
      if (value.backend === "fs") lifecycle.beginDriveChange();
      // The credentials of an object-store Drive go with it.
      else await secretFiles?.removeConfigSecrets(value.config);
      return value;
    },

    /** A sync that is already running counts as success (10 §8.3). */
    "drive-sync": async ({ operationId, driveId, deep, confirmMassDelete }) => {
      const body = {
        ...(deep === undefined ? {} : { deep }),
        ...(confirmMassDelete === undefined ? {} : { confirm_mass_delete: confirmMassDelete }),
      };
      try {
        const { value, replayed } = await call("driveSync", { params: { id: driveId }, body, operationId });
        return handlerResult(ensure(value, isSyncInfo), { replayed });
      } catch (error) {
        if (error?.code === "DOCS_SOURCE_SYNC_RUNNING") {
          return { status: "running", ...pickDetails("DOCS_SOURCE_SYNC_RUNNING", error.details) };
        }
        throw error;
      }
    },

    /** Retries the given files; the renderer already cut the list into batches of at most 500. */
    "files-retry": async ({ operationId, fileIds, mode }) => {
      const body = { file_ids: fileIds, ...(mode ? { mode } : {}) };
      const { value, replayed } = await call("filesRetry", { body, operationId });
      return handlerResult(ensure(value, isBatchResult), { replayed });
    },

    /**
     * "Select all N matching" then retry: pages through the same filter, keeps the files the engine marks retryable and
     * sends them in batches of 500, each with its own idempotency key derived from the operation id.
     */
    "files-retry-matching": async ({ operationId, driveId, path, recursive, states, q, mode }) => {
      const ids = [];
      let cursor;
      for (let page = 0; page < MATCHING_PAGE_CAP; page += 1) {
        const { value } = await call("filesList", {
          params: { id: driveId }, query: { path, recursive, state: states, q, sort: "path", limit: MATCHING_PAGE_LIMIT, cursor },
        });
        ensure(value, isPage);
        for (const item of value.items) if (ensure(item, isFileSummary) && item.retryable === true) ids.push(item.id);
        if (!value.next_cursor) break;
        cursor = value.next_cursor;
      }
      const results = [];
      for (let start = 0, batch = 0; start < ids.length; start += RETRY_BATCH_SIZE, batch += 1) {
        const body = { file_ids: ids.slice(start, start + RETRY_BATCH_SIZE), ...(mode ? { mode } : {}) };
        const { value } = await call("filesRetry", { body, operationId: `${operationId}-${batch}` });
        results.push(...ensure(value, isBatchResult).results);
      }
      return { results, matched: ids.length };
    },

    "drive-tree": async ({ driveId, path }) => {
      const { value } = await call("driveTree", { params: { id: driveId }, query: { path } });
      return ensure(value, isTree);
    },

    "files-list": async ({ driveId, path, recursive, states, q, sort, limit, cursor }) => {
      const { value } = await call("filesList", {
        params: { id: driveId },
        query: { path, recursive, state: states, q, sort, limit, cursor },
      });
      ensure(value, isPage);
      for (const item of value.items) ensure(item, isFileSummary);
      return value;
    },
  };
}
