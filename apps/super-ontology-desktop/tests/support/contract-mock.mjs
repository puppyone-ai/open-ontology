// In-process mock of the docs-api control API, built from the repository contract (contracts/docs-api/*), never
// hand-written (50 §6): every response body is a schema example that this module validates with Ajv before returning.
import fs from "node:fs";
import path from "node:path";
import Ajv2020 from "ajv/dist/2020.js";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");

const DATE_TIME = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/u;
const SAMPLE_UUID = "00000000-0000-4000-8000-000000000001";

export function loadContract(directory = path.join(repoRoot, "contracts/docs-api")) {
  const read = (file) => JSON.parse(fs.readFileSync(path.join(directory, file), "utf8"));
  return {
    schema: read("docs-api.v1.schema.json"),
    errors: read("errors.v1.json"),
    events: read("events.v1.schema.json"),
  };
}

export function createContractValidator(contract = loadContract()) {
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  ajv.addFormat("date-time", { type: "string", validate: (value) => DATE_TIME.test(value) });
  ajv.addSchema(contract.schema);
  const eventValidator = ajv.compile(contract.events);
  const base = contract.schema.$id;
  const defValidator = (name) => {
    const validate = ajv.getSchema(`${base}#/$defs/${name}`);
    if (!validate) throw new Error(`Unknown docs-api definition: ${name}`);
    return validate;
  };
  return {
    ajv,
    /** @returns {{ valid: boolean, errors: string[] }} */
    validateDef(name, value) {
      const validate = defValidator(name);
      const valid = validate(value);
      return { valid, errors: valid ? [] : (validate.errors ?? []).map((error) => `${error.instancePath} ${error.message}`) };
    },
    validateEvent(value) {
      const valid = eventValidator(value);
      return { valid, errors: valid ? [] : (eventValidator.errors ?? []).map((error) => `${error.instancePath} ${error.message}`) };
    },
  };
}

/** Response kinds of x-endpoints that are not JSON definitions: name -> content type. */
export const OPAQUE_RESPONSES = Object.freeze({
  "204": null,
  bytes: "application/octet-stream",
  "text/event-stream": "text/event-stream",
  "text/plain": "text/plain",
});

/** Splits "Page<Drive>" into its envelope and item definition names. */
export function parseTypeName(name) {
  const page = /^Page<(.+)>$/u.exec(name);
  return page ? { page: true, item: page[1] } : { page: false, item: name };
}

function templateToRegExp(template) {
  const source = template.split(/(\{[a-z_]+\})/u)
    .map((part) => (/^\{[a-z_]+\}$/u.test(part) ? "[^/:]+" : part.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&")))
    .join("");
  return new RegExp(`^${source}$`, "u");
}

/**
 * handle({ method, path, headers, body }) -> { status, headers, body }.
 * Routes come from x-endpoints; bodies are schema examples validated before they are returned.
 */
export function createContractMock(contract = loadContract()) {
  const validator = createContractValidator(contract);
  const routes = contract.schema["x-endpoints"].map((endpoint) => ({
    endpoint,
    pattern: templateToRegExp(endpoint.path),
    specificity: endpoint.path.replace(/\{[a-z_]+\}/gu, "").length,
  })).sort((left, right) => right.specificity - left.specificity);
  const replays = new Map();

  const example = (name) => {
    const { page, item } = parseTypeName(name);
    if (page) {
      const items = item === "object" ? [] : [example(item)];
      return { items, next_cursor: null, as_of_seq: 0 };
    }
    if (item === "object") return {};
    const def = contract.schema.$defs[item];
    if (!def) throw new Error(`No definition for ${item}`);
    if (!Array.isArray(def.examples) || def.examples.length === 0) throw new Error(`Definition ${item} has no example`);
    return structuredClone(def.examples[0]);
  };

  const fail = (status, code) => ({
    status,
    headers: { "content-type": "application/json", "OO-Docs-Api": "1.0" },
    body: { code, message: "mock", retryable: false, details: {} },
  });

  return {
    routes: routes.map(({ endpoint }) => endpoint),
    validator,
    example,
    handle({ method, path, headers = {}, body }) {
      const pathname = path.split("?")[0];
      const route = routes.find(({ endpoint, pattern }) => endpoint.method === method && pattern.test(pathname));
      if (!route) return fail(404, "DOCS_ENGINE_NOT_FOUND");
      const { endpoint } = route;
      const key = headers["Idempotency-Key"] ?? headers["idempotency-key"];
      if (endpoint.idempotency_key && !key) return fail(400, "DOCS_ENGINE_BAD_REQUEST");
      if (endpoint.request && endpoint.request !== "object" && body !== undefined) {
        const { valid } = validator.validateDef(parseTypeName(endpoint.request).item, body);
        if (!valid) return fail(400, "DOCS_ENGINE_BAD_REQUEST");
      }
      const replayKey = key ? `${method} ${pathname} ${key}` : null;
      if (replayKey && replays.has(replayKey)) {
        const first = replays.get(replayKey);
        return { ...first, headers: { ...first.headers, "OO-Idempotent-Replay": "true" } };
      }
      const noBody = endpoint.response === "204" || endpoint.response === null;
      const opaque = noBody || Object.hasOwn(OPAQUE_RESPONSES, endpoint.response);
      const response = noBody
        ? { status: 204, headers: {}, body: undefined }
        : {
          status: 200,
          headers: { "content-type": opaque ? OPAQUE_RESPONSES[endpoint.response] : "application/json", "OO-Docs-Api": "1.0" },
          body: opaque ? "" : example(endpoint.response),
        };
      const { page, item } = noBody ? { page: false, item: "object" } : parseTypeName(endpoint.response);
      if (!opaque && item !== "object") {
        const { valid, errors } = validator.validateDef(item, page ? response.body.items[0] : response.body);
        if (!valid) throw new Error(`Mock response for ${method} ${endpoint.path} violates the contract: ${errors.join("; ")}`);
      }
      if (replayKey) replays.set(replayKey, response);
      return response;
    },
  };
}

/** Fills {params} of an x-endpoints path with a sample uuid. */
export function fillPath(template) {
  return template.replace(/\{[a-z_]+\}/gu, SAMPLE_UUID);
}
