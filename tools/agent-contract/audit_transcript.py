#!/usr/bin/env python3
"""Audit execution-tool replies without treating shell exit 0 as query success."""

import argparse
import ast
from collections import Counter
import hashlib
import json
from pathlib import Path
import re


def messages(text):
    question = None
    message = None
    fence = None
    in_block = False
    for line_number, line in enumerate(text.splitlines(), 1):
        marker = re.match(r"^(`{3,}|~{3,})(.*)$", line)
        if marker:
            ticks, suffix = marker.groups()
            if fence is None:
                fence = ticks
                in_block = True
            elif ticks[0] == fence[0] and len(ticks) >= len(fence) and not suffix.strip():
                fence = None
                in_block = False
            elif message is not None:
                message["body"].append((line_number, line))
            continue
        if fence is None:
            heading = re.match(r"^## 问题 (\d+)\s*$", line)
            step = re.match(r"^#### (\d+) (assistant|user|tool)(?: `([^`]+)`)?", line)
            if heading or step:
                if message is not None:
                    yield message
                message = None
                if heading:
                    question = int(heading[1])
                elif question is not None:
                    message = {"question": question, "step": int(step[1]),
                               "role": step[2], "tool": step[3],
                               "line": line_number, "body": []}
        elif in_block and message is not None:
            message["body"].append((line_number, line))
    if message is not None:
        yield message


def structured_objects(lines):
    """Read whole JSON values (including pretty JSON) or single-line Python dicts."""
    decoder = json.JSONDecoder()
    text = "\n".join(line for _, line in lines)
    offsets = []
    offset = 0
    for source_line, line in lines:
        offsets.append((offset, source_line))
        offset += len(line) + 1
    consumed = 0
    for offset, source_line in offsets:
        if offset < consumed:
            continue
        end = text.find("\n", offset)
        line = text[offset:end if end >= 0 else len(text)]
        start = re.search(r"[\{\[]", line)
        if not start:
            continue
        pos = offset + start.start()
        try:
            value, count = decoder.raw_decode(text[pos:])
            consumed = pos + count
        except (ValueError, RecursionError):
            try:
                value = ast.literal_eval(line[start.start():])
                consumed = offset + len(line)
            except (ValueError, SyntaxError, RecursionError):
                continue
        if isinstance(value, dict):
            yield source_line, value


def audit(text):
    questions = {}
    for message in messages(text):
        number = message["question"]
        question = questions.setdefault(number, {
            "question": number, "execution_tool_replies": 0, "nonzero_shell_replies": [],
            "observed_query_ids": [], "truncation_observations": [], "responses": []})
        if message["role"] != "tool" or message["tool"] not in ("terminal", "execute_code"):
            continue
        question["execution_tool_replies"] += 1
        shell = next((int(match[1]) for _, line in message["body"]
                      if (match := re.match(r"exit_code:\s*(-?\d+)", line))), None)
        if shell not in (None, 0):
            question["nonzero_shell_replies"].append({"line": message["line"], "exit": shell})
        for line_number, line in message["body"]:
            for query_id in re.findall(r"\bq_[A-Za-z0-9]+\b", line):
                if query_id not in question["observed_query_ids"]:
                    question["observed_query_ids"].append(query_id)
            if re.search(r"""["']truncated["']\s*:\s*(true|True)|\btrunc True\b|^q_\w+\s+\d+\s+True\b""", line):
                question["truncation_observations"].append({"line": line_number})
        for line, value in structured_objects(message["body"]):
            error = value.get("error") if isinstance(value.get("error"), dict) else value
            is_error = isinstance(error.get("code"), str) and (
                "message" in error or "retryable" in error)
            is_query = "query_id" in value and "rows" in value
            if not is_error and not is_query:
                continue
            event = {"line": line, "step": message["step"], "shell_exit": shell,
                     "query_id": value.get("query_id", error.get("query_id")),
                     "outcome": "error" if is_error else "query_result"}
            if is_error:
                event.update(code=error["code"], masked_by_shell_success=shell == 0)
            else:
                for key in ["duration_ms", "returned_rows", "truncated", "truncation_reasons"]:
                    if key in value:
                        event[key] = value[key]
                event["warning_codes"] = [item.get("code") for item in value.get("warnings", [])
                                          if isinstance(item, dict)]
            question["responses"].append(event)
    for question in questions.values():
        events = question["responses"]
        errors = [event for event in events if event["outcome"] == "error"]
        question["error_counts"] = dict(Counter(event["code"] for event in errors))
        question["shell_masked_errors"] = sum(event["masked_by_shell_success"] for event in errors)
        question["parsed_query_results"] = sum(event["outcome"] == "query_result" for event in events)
        question["truncated_results"] = sum(event.get("truncated") is True for event in events)
    return {
        "protocol": "oo.recorded-agent-usage-audit/v1",
        "source_sha256": hashlib.sha256(text.encode()).hexdigest(),
        "scope": "Parseable terminal/execute_code replies only; not all attempted SQL, live replay, or business acceptance. Observed IDs and truncation markers include wrapper output, not full receipts.",
        "questions": list(questions.values()),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("transcript", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = audit(args.transcript.read_text())
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=True) + "\n")
    for question in result["questions"]:
        print(json.dumps({key: value for key, value in question.items()
                          if key not in ("responses", "observed_query_ids")}))


if __name__ == "__main__":
    main()
