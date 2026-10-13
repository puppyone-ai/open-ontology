#!/usr/bin/env python3
"""Append one bounded StarRocks dialogue to two immutable failed transcripts."""

import argparse
import hashlib
import importlib.util
import json
import pathlib
import subprocess
import urllib.request


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verified_json(path, digest):
    if sha(path) != digest:
        raise ValueError("input digest changed")
    return json.loads(path.read_text())


def model_module(path, expected):
    if sha(path) != expected:
        raise ValueError("shared verifier changed")
    spec = importlib.util.spec_from_file_location("shared_dialogue", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def compact_context(cfg, step_index, queries):
    names = (("agg_metrics", "dup_partition_sums"), ("dup_events",), ("uniq_dim", "pk_accounts"))[step_index]
    documents = []
    for name in names:
        item = verified_json(pathlib.Path(cfg["public_documents"][name]["file"]), cfg["public_documents"][name]["file_sha256"])
        if item["sha256"] != cfg["public_documents"][name]["document_sha256"]:
            raise ValueError("public document changed")
        documents.append({"path": item["path"], "sha256": item["sha256"], "text": item["text"]})
    def actual(query_id):
        body = queries[query_id]
        return {"query_id": query_id, "columns": body["columns"], "rows": body["rows"],
                "warning_codes": [warning["code"] for warning in body.get("warnings", [])]}
    selected = ((cfg["qty_query_id"],), (cfg["bucket_query_id"], cfg["sum_query_id"]), ())[step_index]
    context = {"source": "SHA-bound actual public OO CLI observations; original raw transcripts retained unchanged",
               "role_pack_digest": cfg["pack_digest"], "public_documents": documents,
               "prior_successful_queries": [actual(query_id) for query_id in selected],
               "first_turn_confirmed": [{"query_id": item["query_id"], "value": item["value"]}
                                        for item in cfg["first_turn_cells"]]}
    if step_index == 0:
        context["last_actual_error"] = {"query_id": cfg["terminal_query_id"],
                                        "gateway_code": "INTERNAL", "postgres_sqlstate": "42803",
                                        "reason": "A selected nonaggregate column was not grouped."}
    return context


def fact_value(value):
    if isinstance(value, dict):
        value = value.get("value")
    if isinstance(value, bool) or not isinstance(value, (str, int)):
        raise ValueError("fact value type")
    return str(value)


def verify_answer(answer, expected, queries):
    report = json.loads(answer)
    if set(report) != {"facts", "evidence"} or not isinstance(report["facts"], dict) or not isinstance(report["evidence"], dict):
        raise ValueError("named fact envelope")
    if set(report["facts"]) != set(expected) or set(report["evidence"]) != set(expected):
        raise ValueError("named fact coverage")
    for name, wanted in expected.items():
        if fact_value(report["facts"][name]) != str(wanted):
            raise ValueError("independent fact mismatch: " + name)
        link = report["evidence"][name]
        if not isinstance(link, dict) or not isinstance(link.get("query_id"), str):
            raise ValueError("query link absent")
        body = queries.get(link["query_id"])
        if body is None:
            raise ValueError("query link not successful")
        columns = [column["name"] for column in body["columns"]]
        if name in {"may_count", "june_count"}:
            month = "2026-05" if name == "may_count" else "2026-06"
            if link.get("month") != month or columns.count("bucket") != 1:
                raise ValueError("month evidence absent")
            if "column" in link and "count_column" in link and link["column"] != link["count_column"]:
                raise ValueError("conflicting month count columns")
            count_column = link.get("column", link.get("count_column"))
            if columns.count(count_column) != 1:
                raise ValueError("month count column absent")
            bucket_at, count_at = columns.index("bucket"), columns.index(count_column)
            actual = sum(int(row[count_at]) for row in body["rows"] if row[bucket_at][:7] == month)
            if str(actual) != str(wanted):
                raise ValueError("month evidence mismatch")
        else:
            column = link.get("column")
            row_index = link.get("row")
            if columns.count(column) != 1 or type(row_index) is not int or not 0 <= row_index < len(body["rows"]):
                raise ValueError("cell evidence absent")
            if str(body["rows"][row_index][columns.index(column)]) != str(wanted):
                raise ValueError("cell evidence mismatch")
    for letter in ("a", "b"):
        names = ("part_" + letter, "count_" + letter, "amount_" + letter)
        if all(name in expected for name in names):
            positions = {(report["evidence"][name]["query_id"], report["evidence"][name]["row"]) for name in names}
            if len(positions) != 1:
                raise ValueError("rollup fields came from different rows")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--preflight", action="store_true")
    parser.add_argument("--remaining-only", action="store_true")
    args = parser.parse_args()
    cfg = json.loads(pathlib.Path(args.config).read_text())
    original = pathlib.Path(cfg["original_transcript"])
    continuation = pathlib.Path(cfg["continuation_transcript"])
    first = verified_json(original, cfg["original_sha256"])
    second = verified_json(continuation, cfg["continuation_sha256"])
    if len(first) != 17 or len(second) != 12:
        raise ValueError("history length")
    shared = model_module(pathlib.Path(cfg["shared_verifier"]), cfg["shared_sha256"])
    if cfg["model"] != "grok-4.7" or cfg["max_tool_rounds"] != 4 or cfg["max_tokens"] != 1200 or cfg["model_origin"] != "http://127.0.0.1:8317":
        raise ValueError("continuation configuration")
    original_error = next(event for event in first if event.get("tool") == "query" and event.get("exit") == 2)
    current_error = next(event for event in second if event.get("tool") == "query" and event.get("response", {}).get("code") == "INTERNAL")
    if original_error["response"]["code"] != "COLUMN_NOT_ALLOWED" or current_error["response"]["query_id"] != cfg["terminal_query_id"]:
        raise ValueError("history refusal changed")
    if shared.model_view("query", current_error["response"]).get("code") != "INTERNAL":
        raise ValueError("shared error view lost code")
    system = "Use OO tools autonomously to inspect role knowledge, schema, freshness, validate and query. Preserve exact decimal and original source type/quality warnings. Never infer a value without evidence. Include query_id in factual answers. You cannot use direct DB or source credentials."
    queries = {event["response"]["query_id"]: event["response"] for event in first + second
               if event.get("tool") == "query" and event["exit"] == 0}
    if args.remaining_only:
        previous = verified_json(pathlib.Path(cfg["previous_attempt"]), cfg["previous_attempt_sha256"])
        for event in previous:
            if event.get("tool") == "query" and event["exit"] == 0:
                queries[event["response"]["query_id"]] = event["response"]
        for index in (0, 1):
            answers = [event["model"]["choices"][0]["message"]["content"] for event in previous
                       if event.get("step") == index and "model" in event
                       and not event["model"]["choices"][0]["message"].get("tool_calls")]
            if len(answers) != 1:
                raise ValueError("previous answer count")
            verify_answer(answers[0], cfg["steps"][index]["expected_named_facts"], queries)
    proof = verified_json(pathlib.Path(cfg["prefix_proof"]), cfg["prefix_proof_sha256"])
    if len(proof["completed_turn0_query_proof"]) != 3 or proof["turn1_exact_sum"]["value"] != cfg["original_sum"]:
        raise ValueError("successful prefix proof")
    for item in proof["completed_turn0_query_proof"]:
        if queries[item["query_id"]]["rows"][item["row"]][0] != item["value"]:
            raise ValueError("prefix cell changed")
    if proof["completed_turn0_query_proof"] != cfg["first_turn_cells"]:
        raise ValueError("first turn summary changed")
    tools = [{"type": "function", "function": {"name": name, "description": "Read actual role-bound OO Gateway " + name,
              "parameters": {"type": "object", "properties": {key: {"type": value} for key, value in fields.items()},
                             "required": list(fields), "additionalProperties": False}}}
             for name, fields in shared.TOOLS.items()]
    contexts = [compact_context(cfg, index, queries) for index in range(len(cfg["steps"]))]
    def initial_messages(index):
        return [{"role": "system", "content": system},
                {"role": "user", "content": "Actual public context: " + json.dumps(contexts[index], separators=(",", ":"))},
                {"role": "user", "content": cfg["steps"][index]["prompt"]}]
    token = pathlib.Path(cfg["gateway_token_file"])
    shared.private(str(token))
    bearer = shared.private(cfg["model_bearer_file"])
    out = pathlib.Path(args.out)
    if out.exists():
        raise ValueError("output exists")
    if args.preflight:
        wrong = next(event["model"]["choices"][0]["message"]["content"] for event in second
                     if event.get("turn") == 1 and "model" in event and not event["model"]["choices"][0]["message"].get("tool_calls"))
        try:
            verify_answer(wrong, cfg["steps"][1]["expected_named_facts"], queries)
        except (ValueError, json.JSONDecodeError):
            pass
        else:
            raise ValueError("old daily list unexpectedly accepted")
        indices = (2,) if args.remaining_only else range(len(cfg["steps"]))
        sizes = [len(json.dumps({"model": "grok-4.7", "reasoning_effort": "low", "messages": initial_messages(index),
                                 "tools": tools, "max_tokens": 1200}).encode()) for index in indices]
        print(json.dumps({"preflight": "ok", "initial_request_bytes": sizes, "queries": len(queries),
                          "wrong_daily_answer_rejected": True, "model_calls": 0}))
        return
    out.mkdir(mode=0o700)
    common = ["--json", "--gateway", cfg["gateway"], "--transport", cfg["transport"],
              "--auth", "external", "--token-file", cfg["gateway_token_file"]]
    appended = []
    indices = (2,) if args.remaining_only else range(len(cfg["steps"]))
    for index in indices:
        step = cfg["steps"][index]
        messages = initial_messages(index)
        for round_number in range(3 if args.remaining_only else 4):
            payload = {"model": "grok-4.7", "reasoning_effort": "low", "messages": messages,
                       "tools": tools, "tool_choice": "required" if index != 1 and round_number == 0 else "auto",
                       "max_tokens": 1200}
            request = urllib.request.Request(cfg["model_origin"] + "/v1/chat/completions",
                      data=json.dumps(payload).encode(), headers={"Content-Type": "application/json",
                      "Authorization": "Bearer " + bearer})
            with urllib.request.urlopen(request, timeout=120) as response:
                model = json.load(response)
            message = model["choices"][0]["message"]
            messages.append(message)
            appended.append({"step": index, "round": round_number, "model": model})
            (out / "transcript.json").write_text(json.dumps(appended, indent=2) + "\n")
            calls = message.get("tool_calls") or []
            if not calls:
                verify_answer(message.get("content") or "", step["expected_named_facts"], queries)
                break
            for call in calls:
                name = call["function"]["name"]
                arguments = json.loads(call["function"]["arguments"])
                if name not in shared.TOOLS or set(arguments) != set(shared.TOOLS[name]):
                    raise ValueError("unexpected tool")
                command = {"knowledge_cat": ["knowledge", "cat", arguments.get("path", "")],
                           "knowledge_search": ["knowledge", "search", arguments.get("query", "")],
                           "query": ["query", arguments.get("sql", ""), "--max-rows", "100", "--timeout-ms", "10000"],
                           "describe": ["describe", arguments.get("table", "")],
                           "validate": ["validate", arguments.get("sql", "")],
                           "tables": ["tables"], "freshness": ["freshness"]}[name]
                result = subprocess.run([cfg["oo_cli"]] + command + common,
                                        capture_output=True, text=True, timeout=45)
                body = json.loads(result.stdout)
                if name == "query" and result.returncode == 0:
                    queries[body["query_id"]] = body
                view = shared.model_view(name, body)
                appended.append({"step": index, "tool": name, "arguments": arguments,
                                 "exit": result.returncode, "response": body, "model_view": view})
                (out / "transcript.json").write_text(json.dumps(appended, indent=2) + "\n")
                if result.returncode and not (name in {"query", "validate"} and body.get("class") == "gateway"
                                              and body.get("code") in {"COLUMN_NOT_ALLOWED", "STATEMENT_NOT_ALLOWED", "FUNCTION_NOT_ALLOWED"}):
                    raise ValueError("new OO CLI error: " + name)
                messages.append({"role": "tool", "tool_call_id": call["id"],
                                 "content": json.dumps({"exit": result.returncode, "response": view})})
        else:
            raise ValueError("tool round budget exhausted")
    print(json.dumps({"outcome": "continued", "steps": len(indices), "events": len(appended)}))


if __name__ == "__main__":
    main()
