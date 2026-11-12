"""Bounded model capture and publication-pinned, read-only Gateway execution."""
from concurrent.futures import ThreadPoolExecutor
from decimal import Decimal, InvalidOperation
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time

import run as ab

# Closed API codes only; never retain CLI stderr or a remote error message.
QUERY_FAILURE_CODES = frozenset((
    "UNAUTHENTICATED", "SQL_PARSE_ERROR", "STATEMENT_NOT_ALLOWED",
    "FUNCTION_NOT_ALLOWED", "UNQUALIFIED_RELATION", "COST_TOO_HIGH",
    "TIMEOUT", "TABLE_PAUSED", "QUERY_TOO_SHORT", "RATE_LIMITED",
    "OVERLOADED", "INTERNAL", "AUDIT_UNAVAILABLE", "IDEMPOTENCY_CONFLICT",
    "CONFLICT", "VERSION_UNSUPPORTED", "SCHEMA_INVALID",
))

spec = importlib.util.spec_from_file_location(
    "ab_model_egress", ab.ROOT / "tools/model-egress/model_egress.py")
egress = importlib.util.module_from_spec(spec)
spec.loader.exec_module(egress)


def typed_result(result):
    """Use returned types, never gold types, and never round through floats."""
    aliases = {
        "int16": "int64", "int32": "int64", "int64": "int64",
        "int2": "int64", "int4": "int64", "int8": "int64",
        "integer": "int64", "bigint": "int64",
        "numeric": "decimal", "decimal": "decimal",
        "decimal128": "decimal", "decimal256": "decimal",
        "text": "text", "string": "text", "utf8": "text",
        "bool": "bool", "boolean": "bool",
    }
    columns = []
    for column in result["columns"]:
        kind = aliases.get(column["type"])
        if kind is None:
            raise ab.Invalid("gateway_type_unsupported")
        columns.append({"name": column["name"], "type": kind})
    rows = []
    if result["returned_rows"] != len(result["rows"]):
        raise ab.Invalid("gateway_row_count")
    for row in result["rows"]:
        if len(row) != len(columns):
            raise ab.Invalid("gateway_row_width")
        cells = []
        for value, column in zip(row, columns):
            if value is None:
                cells.append({"presence": "null"})
                continue
            if not isinstance(value, str):
                raise ab.Invalid("gateway_cell_type")
            kind = column["type"]
            cell = {"presence": "value"}
            if kind == "decimal":
                try:
                    number = Decimal(value)
                except InvalidOperation:
                    raise ab.Invalid("gateway_decimal") from None
                if not number.is_finite():
                    raise ab.Invalid("gateway_decimal")
                sign, digits, exponent = number.as_tuple()
                coefficient = "".join(map(str, digits))
                if exponent > 0:
                    coefficient += "0" * exponent
                cell.update(negative=bool(sign), coefficient=coefficient,
                            scale=max(0, -exponent))
            elif kind == "int64":
                if not re.fullmatch(r"-?(0|[1-9][0-9]*)", value):
                    raise ab.Invalid("gateway_integer")
                cell["value"] = value
            elif kind == "bool":
                if value not in ("true", "false"):
                    raise ab.Invalid("gateway_boolean")
                cell["value"] = value == "true"
            else:
                cell["value"] = value
            cells.append(cell)
        rows.append(cells)
    return {"kind": "rows", "columns": columns, "rows": rows,
            "completion": "partial" if result["truncated"] else "complete",
            "warnings": result["warnings"]}


def score(question, response):
    if response.get("kind") == "gateway_denied":
        if question["track"] == "accuracy":
            return {"pass": False, "no_output": False, "reason": "gateway_denial",
                    "code": response["gateway_result"]["code"]}
        return {"pass": question["expected_kind"] == "denied", "no_output": False,
                "reason": "gateway_denial", "code": response["gateway_result"]["code"]}
    if response.get("kind") == "query_failed":
        scored = {"pass": False, "no_output": False, "reason": "query_failed"}
        if response.get("gateway_result", {}).get("code") in QUERY_FAILURE_CODES:
            scored["code"] = response["gateway_result"]["code"]
        return scored
    if question["expected_kind"] == "denied":
        return {"pass": False, "no_output": False, "reason": "expected_denial"}
    try:
        actual = typed_result(response["gateway_result"])
    except ab.Invalid as error:
        if str(error) != "gateway_type_unsupported":
            raise
        return {"pass": False, "no_output": False, "reason": "unsupported_result_type"}
    scored = ab.compare_rows(question, actual)
    scored["warnings"] = actual["warnings"]
    scored["freshness"] = response["gateway_result"].get("freshness")
    return scored


def gateway(cli, profile, binding, sql):
    config = Path(os.environ.get("OO_CONFIG_DIR", Path.home() / ".config/open-ontology"))
    pointer = config / "active-profile.json"
    active = ab.load(pointer)["profile"] if pointer.exists() else "default"
    if active != profile:
        raise ab.Invalid("activate_requested_profile")
    args = [str(cli), "query", "--json",
            "--expected-publication-id", binding["publication_id"],
            "--expected-data-vector-sha256", binding["data_vector_sha256"]]
    try:
        # The CLI intentionally reserves stdin for credentials. Use its
        # existing SQL-file interface, not SQL/secret material in argv.
        with tempfile.NamedTemporaryFile(mode="w", dir=ab.ROOT / "evidence") as stream:
            stream.write(sql)
            stream.flush()
            result = subprocess.run([*args, "-f", stream.name],
                                    text=True, capture_output=True, timeout=45)
    except (subprocess.TimeoutExpired, OSError):
        return None
    if len(result.stdout.encode()) > 1_048_576:
        raise ab.Invalid("gateway_response_budget")
    if result.returncode:
        try:
            error = json.loads(result.stdout)
        except ValueError:
            return None
        if not isinstance(error, dict):
            return None
        if (error.get("class") == "gateway" and error.get("code") in (
                "COLUMN_NOT_ALLOWED", "RELATION_NOT_FOUND", "ENUMERATION_REFUSED")
                and isinstance(error.get("query_id"), str) and error["query_id"]):
            return {"denied": True, "code": error["code"], "query_id": error["query_id"]}
        if error.get("class") == "gateway" and error.get("code") in QUERY_FAILURE_CODES:
            failure = {"failed": True, "code": error["code"]}
            query_id = error.get("query_id")
            if isinstance(query_id, str) and re.fullmatch(r"q_[0-9A-Za-z]{6,40}", query_id):
                failure["query_id"] = query_id
            return failure
        return None
    value = json.loads(result.stdout, object_pairs_hook=ab.object_pairs)
    if (value.get("publication_binding") != {
            k: binding[k] for k in ("publication_id", "data_vector_sha256")}
            or any(value.get(k) != binding[k] for k in ("knowledge_version", "policy_version"))):
        raise ab.Invalid("publication_drift")
    try:
        typed_result(value)
    except ab.Invalid as error:
        if str(error) != "gateway_type_unsupported":
            raise
        # Preserve the real receipt. An unsupported answer type is a failed
        # answer, not permission to coerce values or discard the whole run.
    return value


def model_response(client, envelope, seed, retries):
    request = {"model": client.model["model"], "seed": seed, "stream": False,
               "max_completion_tokens": 4096, "messages": [
                   {"role": "system", "content":
                    'Return JSON only: {"sql":"SELECT ..."} or {"refusal":true}. '
                    'Use a read-only query. Do not include markdown.'},
                   {"role": "user", "content": ab.canonical(envelope)}]}
    for attempt in range(retries + 1):
        response = None
        try:
            response = list(client.call(request))[0]
            # Endpoint files can be secret too; the shared egress checks keys.
            if client.url.geturl() in ab.canonical(response):
                raise ab.Invalid("model_secret_echo")
            content = response["choices"][0]["message"]["content"]
            if not isinstance(content, str) or not content.strip():
                return {"kind": "no_output", "reason": "empty_response",
                        "model_response": response, "attempts": attempt + 1}
            parsed = json.loads(content, object_pairs_hook=ab.object_pairs)
            if parsed == {"refusal": True}:
                return {"kind": "denied", "code": "POLICY_DENIED",
                        "model_response": response, "attempts": attempt + 1}
            if (set(parsed) != {"sql"} or not isinstance(parsed["sql"], str)
                    or not parsed["sql"].strip() or len(parsed["sql"]) > 65536):
                raise ValueError()
            return {"kind": "gateway", "sql": parsed["sql"],
                    "sql_sha256": ab.digest(parsed["sql"].encode()),
                    "model_response": response, "attempts": attempt + 1}
        except egress.Refusal as error:
            if error.code not in ("UPSTREAM_ERROR", "UPSTREAM_TIMEOUT") or attempt == retries:
                return {"kind": "no_output", "reason": "model_refused", "attempts": attempt + 1}
        except (OSError, TimeoutError):
            if attempt == retries:
                return {"kind": "no_output", "reason": "model_transport", "attempts": attempt + 1}
        except (ValueError, KeyError, TypeError, IndexError):
            return {"kind": "no_output", "reason": "model_protocol",
                    "model_response": response, "attempts": attempt + 1}
        time.sleep(min(2 ** attempt, 4))
    raise ab.Invalid("retry_state")


def capture(args, dataset_path):
    dataset_path = dataset_path.resolve()
    if not dataset_path.is_relative_to(ab.ROOT):
        raise ab.Invalid("dataset_scope")
    dataset = ab.load(dataset_path)
    ab.validate_dataset(dataset)
    if args.model != dataset["model"]:
        raise ab.Invalid("model_identity")
    binding = dataset["publication"]
    if (not isinstance(binding, dict) or set(binding) != {
            "publication_id", "data_vector_sha256", "knowledge_version", "policy_version"}
            or not re.fullmatch(r"pub1_[0-9a-f]{64}", binding["publication_id"])
            or not re.fullmatch(r"[0-9a-f]{64}", binding["data_vector_sha256"])
            or not all(isinstance(v, str) and v for v in binding.values())):
        raise ab.Invalid("live_publication_binding_required")
    refs = {key: ab.pinned(dataset_path.parent, ref)
            for key, ref in dataset["inputs"].items() if key != "responses"}
    schema, pack = ab.load(refs["schema"]), ab.load(refs["pack"])
    ab.validate_role(dataset, pack)
    if not 1 <= args.concurrency <= 4 or not 1 <= args.timeout <= 120 or not 0 <= args.retries <= 2:
        raise ab.Invalid("live_budget")
    model = {"provider": "openai-compatible", "model": args.model,
             "base_url": {"secret_ref": "file:" + str(args.model_endpoint_file.absolute())},
             "key": {"secret_ref": "file:" + str(args.model_key_file.absolute())}}
    # Validate files and authenticated publication before making any paid call.
    try:
        client = egress.Egress(model, None, timeout=args.timeout)
    except (egress.Refusal, OSError):
        raise ab.Invalid("model_source_invalid") from None
    client.close()
    probe = gateway(args.oo, args.profile, binding, "SELECT 1 AS value")
    if probe is None or probe.get("denied") or probe.get("failed"):
        raise ab.Invalid("publication_probe_failed")
    # Execute every gold using the very same authenticated profile as both
    # arms. A curated label alone cannot establish runtime answerability.
    for question in dataset["questions"]:
        if question["track"] != "accuracy":
            continue
        result = gateway(args.oo, args.profile, binding, question["gold_sql"])
        if result is None or result.get("denied") or result.get("failed"):
            raise ab.Invalid("accuracy_not_answerable")
        if not ab.compare_rows(question, typed_result(result))["pass"]:
            raise ab.Invalid("accuracy_gold_drift")
    output = args.output.resolve()
    if not output.is_relative_to(ab.ROOT / "evidence"):
        raise ab.Invalid("evidence_scope")
    output.mkdir(mode=0o700, parents=True, exist_ok=False)

    def trial(pair):
        arm, question = pair
        envelope = ab.prompt(question, arm, schema, pack,
                             dataset.get("context_format", "artifact-v1"))
        client = egress.Egress(model, None, timeout=args.timeout)
        try:
            response = model_response(client, envelope, dataset["seed"], args.retries)
        finally:
            client.close()
        response["prompt_sha256"] = ab.digest(ab.canonical(envelope).encode())
        ab.write(output / (arm + "-" + question["id"] + "-model.json"), response)
        if response["kind"] == "gateway":
            result = gateway(args.oo, args.profile, binding, response["sql"])
            if result is None:
                response["kind"] = "query_failed"
            else:
                response["gateway_result"] = result
                if result.get("denied"):
                    response["kind"] = "gateway_denied"
                elif result.get("failed"):
                    response["kind"] = "query_failed"
        return arm + ":" + question["id"], response

    pairs = [(arm, q) for q in dataset["questions"] for arm in ab.ARMS]
    with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        responses = dict(pool.map(trial, pairs))
    recording = {"version": "oo.ab.recorded/v1", "origin": "live-model-gateway",
                 "provenance": "captured model responses and publication-pinned Gateway results",
                 "model": dataset["model"], "seed": dataset["seed"],
                 "dataset_sha256": ab.digest(ab.canonical(
                     {k: v for k, v in dataset.items() if k != "inputs"}).encode()),
                 "responses": responses}
    ab.write(output / "responses.json", recording)
    dataset["inputs"] = {key: {"path": os.path.relpath(path, output),
                              "sha256": ab.digest(path.read_bytes())}
                         for key, path in refs.items()}
    dataset["inputs"]["responses"] = {"path": "responses.json",
                                     "sha256": ab.digest((output / "responses.json").read_bytes())}
    ab.write(output / "dataset.json", dataset)
    return ab.run(output / "dataset.json", output / "scored")
