"""Bounded real-model gold dialogue through the native CLI, never source SQL."""
import argparse
from collections import Counter
from decimal import Decimal, InvalidOperation
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import time
import traceback

from runner import ROOT, checksums, write_json

MODEL = {
    "provider": "openai-compatible",
    "base_url": {"secret_ref": "model-endpoint-9r"},
    "key": {"secret_ref": "model-key-9r-sol"},
    "model": "gpt-6.1-sol",
}
QUESTIONS = ROOT / "testing/engines/gold/questions.json"
MAX_CALLS = 160
MAX_TURNS = 12
BUILDER_CALL_LIMIT = 6
# Five complete native describe responses include repeated capability catalogs.
# Bytes + proxy allowance + output remain below the plan's 272k context floor.
MAX_REQUEST_BYTES = 250000
MAX_RESERVED_TOKENS = 12_000_000
MAX_OUTPUT = 2048
TOOLS = [{"type": "function", "function": {
    "name": "oo",
    "description": "Native read-only Open Ontology CLI. No shell or filesystem access.",
    "parameters": {"type": "object", "properties": {
        "command": {"type": "string", "enum": [
            "login", "whoami", "tables", "describe", "agent-skill", "query"]},
        "argument": {"type": "string"}},
        "required": ["command"], "additionalProperties": False}}}]


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def redact_material(text):
    text = re.sub(r"OO-[0-9A-Z]{4}-[0-9A-Z]{4}", "[ENROLLMENT_REDACTED]", text)
    return re.sub(r"ooct_[A-Za-z0-9_-]{43}", "[TOKEN_REDACTED]", text)


def error_code(exc):
    text = getattr(exc, "code", str(exc))
    return text if isinstance(text, str) and re.fullmatch(r"[A-Z][A-Z0-9_]{0,63}", text) else None


def cell(value):
    if value is None:
        return None
    if isinstance(value, (bool, dict, list, float)):
        raise ValueError("ANSWER_CELL_TYPE")
    value = str(value)
    try:
        number = Decimal(value)
        if number.is_finite():
            if not number:
                return "0"
            text = format(number, "f")
            return text.rstrip("0").rstrip(".") if "." in text else text
    except InvalidOperation:
        pass
    return value


def classify(answer, gold, *, truncated=False, refused=False, queried=False):
    """Keep NULL provenance and failure categories separate from equality."""
    rows = answer.get("rows") if isinstance(answer, dict) else None
    null = isinstance(rows, list) and any(
        isinstance(row, list) and any(v is None for v in row) for row in rows)
    result = {"equal": False, "null": null, "truncated": truncated, "refused": refused}
    if truncated:
        result["status"] = "truncated"
    elif refused:
        result["status"] = "refused"
    elif not queried:
        result["status"] = "unverified"
    elif not isinstance(rows, list) or any(not isinstance(row, list) for row in rows):
        result["status"] = "invalid_answer"
    else:
        try:
            actual = [[cell(v) for v in row] for row in rows]
            result["equal"] = actual == gold
            result["status"] = "correct" if result["equal"] else "mismatch"
        except ValueError:
            result["status"] = "invalid_answer"
    return result


def cli_arguments(call):
    if not isinstance(call, dict) or set(call) - {"command", "argument"}:
        raise ValueError("CLI_TOOL_SCHEMA")
    command, argument = call.get("command"), call.get("argument", "")
    if not isinstance(argument, str) or len(argument.encode()) > 65536:
        raise ValueError("CLI_TOOL_ARGUMENT")
    if command == "login":
        if not re.fullmatch(r"OO-[0-9A-Z]{4}-[0-9A-Z]{4}", argument):
            raise ValueError("CLI_LOGIN_CODE")
        return [command, argument]
    if command in ("query", "describe"):
        if not argument or argument.startswith("-") or "\x00" in argument:
            raise ValueError("CLI_TOOL_ARGUMENT")
        return [command, argument]
    if command not in ("whoami", "tables", "agent-skill") or argument:
        raise ValueError("CLI_TOOL_COMMAND")
    return [command]


class Model:
    def __init__(self, out):
        self.module = load_module("t6_egress", ROOT / "tools/model-egress/model_egress.py")
        self.egress = self.module.Egress(MODEL, out / "usage.jsonl", rpm=120, timeout=120)
        self.calls = self.reserved = 0
        self.deadline = time.monotonic() + 3600
        self.out = out

    def call(self, messages):
        request = {"model": MODEL["model"], "messages": messages, "tools": TOOLS,
                   "max_tokens": MAX_OUTPUT, "stream": False}
        size = len(json.dumps(request).encode())
        reserve = size + MAX_OUTPUT + 10000
        if size > MAX_REQUEST_BYTES:
            raise ValueError("MODEL_REQUEST_BUDGET")
        if self.calls >= MAX_CALLS:
            raise ValueError("MODEL_CALL_LIMIT")
        if self.reserved + reserve > MAX_RESERVED_TOKENS:
            raise ValueError("MODEL_TOKEN_RESERVATION")
        if time.monotonic() >= self.deadline:
            raise ValueError("MODEL_DEADLINE")
        self.calls += 1
        self.reserved += reserve
        frames = list(self.egress.call(request))
        if len(frames) != 1 or not isinstance(frames[0], dict):
            raise ValueError("MODEL_PROTOCOL")
        # The endpoint is private too; never persist it even if echoed.
        encoded = self.egress.safe(frames[0])
        if self.egress.url.geturl().encode() in encoded:
            raise ValueError("MODEL_ENDPOINT_ECHO")
        (self.out / f"model-{self.calls:03d}.json").write_text(
            redact_material(encoded.decode()) + "\n")
        return frames[0]

    def close(self):
        self.egress.close()


def bind_agent(model, cli, prompt, out):
    messages = [
        {"role": "system", "content": (
            "Bind the installed native oo CLI using the share prompt. Do not install anything. "
            "The harness fixes Gateway URL and TLS files. Call oo login with argument set "
            "to the enrollment code; it is delivered on stdin, never argv. Then call oo "
            "whoami to verify. Stop and report failures honestly. No SQL is needed yet.")},
        {"role": "user", "content": prompt}]
    login = verified = False
    transcript = []
    try:
        for _ in range(6):
            response = model.call(messages)
            choice = response["choices"][0]
            message = choice["message"]
            if choice.get("finish_reason") == "length" or message.get("refusal"):
                raise ValueError("BIND_MODEL_REFUSED_OR_TRUNCATED")
            messages.append(message)
            calls = message.get("tool_calls", [])
            if not calls or len(calls) > 2:
                break
            for call in calls:
                if call["function"]["name"] != "oo":
                    raise ValueError("BIND_TOOL")
                args = cli_arguments(json.loads(call["function"]["arguments"]))
                if args[0] not in ("login", "whoami") or (args[0] == "login" and login):
                    raise ValueError("BIND_TOOL")
                result = cli(args)
                transcript.append({"command": args[0], "output": result})
                messages.append({"role": "tool", "tool_call_id": call["id"],
                                 "content": json.dumps(result)})
                if args[0] == "login":
                    login = result["exit"] == 0
                    if not login:
                        raise ValueError("BIND_LOGIN_FAILED")
                else:
                    verified = login and result["exit"] == 0
            if verified:
                return redact_material(prompt)
        raise ValueError("BIND_NOT_VERIFIED")
    finally:
        write_json(out / "binding.json", {"login": login, "verified": verified,
                                        "tools": transcript})


def dialogue(model, cli, prompt, question, gold, out):
    messages = [
        {"role": "system", "content": (
            "Answer using only the supplied Open Ontology CLI. The connection is already "
            "bound in an isolated profile. Never invent values or replace NULL by zero. "
            "Use metadata commands as the share prompt instructs. Return only JSON "
            '{"rows":[["cell",null]]}, preserving query row/column order and exact '
            "numeric strings; if refused return {\"refused\":true}.")},
        {"role": "user", "content": prompt + "\nQuestion: " + question["question"]},
    ]
    answer, truncated, refused, queried = {}, False, False, False
    transcript = []
    error = None
    try:
        for _ in range(MAX_TURNS):
            response = model.call(messages)
            choice = response["choices"][0]
            message = choice["message"]
            truncated |= choice.get("finish_reason") == "length"
            refused |= bool(message.get("refusal"))
            if truncated or refused:
                break
            messages.append(message)
            calls = message.get("tool_calls", [])
            if not calls:
                answer = json.loads(message.get("content") or "{}")
                refused |= isinstance(answer, dict) and answer.get("refused") is True
                break
            if len(calls) > 8:
                raise ValueError("TOOL_BUDGET")
            for call in calls:
                if call["function"]["name"] != "oo":
                    raise ValueError("TOOL_NAME")
                args = json.loads(call["function"]["arguments"])
                result = cli(cli_arguments(args))
                transcript.append({"input": args, "output": result})
                body = result.get("body", {})
                if isinstance(body, dict):
                    truncated |= body.get("truncated") is True
                if args["command"] == "query":
                    queried |= (result["exit"] == 0 and isinstance(body, dict)
                                and not body.get("error") and isinstance(body.get("rows"), list))
                messages.append({"role": "tool", "tool_call_id": call["id"],
                                 "content": json.dumps(result)})
        else:
            error = "QUESTION_TURN_LIMIT"
    except Exception as exc:
        # Provider/CLI exception messages can contain endpoints or credentials.
        error = error_code(exc) or type(exc).__name__
    report = dict(question, **classify(answer, gold, truncated=truncated,
                                      refused=refused, queried=queried),
                  answer=answer, gold=gold, error=error, tools=transcript,
                  tool_failures=sum(r["output"]["exit"] != 0 or
                                    (isinstance(r["output"].get("body"), dict)
                                     and bool(r["output"]["body"].get("error")))
                                    for r in transcript))
    if error:
        report.update(status="failed", equal=False)
    write_json(out / (question["id"] + ".json"),
               json.loads(redact_material(json.dumps(report))))
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--engine", choices=["postgres", "mysql", "oracle"], default="postgres")
    parser.add_argument("--scale", type=int, default=2_000_000)
    parser.add_argument("--publisher", choices=["reviewed", "fixture"], default="reviewed")
    parser.add_argument("--execute", action="store_true", help="authorize bounded paid model calls")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9-]{0,100}", args.run_id):
        parser.error("invalid run id")
    if not args.execute or not 1 <= args.scale <= 10_000_000:
        parser.error("--execute and a valid fixture scale are required")
    os.umask(0o077)
    out = ROOT / "evidence" / args.run_id
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    preflight = {"model": MODEL["model"], "engine": args.engine,
                 "questions": 20, "max_calls": MAX_CALLS, "max_output_per_call": MAX_OUTPUT,
                 "max_turns_per_question": MAX_TURNS,
                 "max_reserved_tokens": MAX_RESERVED_TOKENS, "max_request_bytes": MAX_REQUEST_BYTES,
                 "deadline_seconds": 3600, "currency_cost": None, "retries": 0,
                 "scale": args.scale, "publisher": args.publisher,
                 "builder_call_limit": BUILDER_CALL_LIMIT if args.publisher == "reviewed" else 0,
                 "max_total_model_calls": MAX_CALLS + (BUILDER_CALL_LIMIT if args.publisher == "reviewed" else 0),
                 "t5_full_selfbuild": False}
    write_json(out / "preflight.json", preflight)
    print(json.dumps(preflight), flush=True)
    model = fixture = None
    report = {"status": "failed", "engine": args.engine, "scale": args.scale,
              "publisher": preflight["publisher"], "questions": [], "containers_left": "unmeasured"}
    stage = "credentials"
    def interrupt(signum, frame):
        raise KeyboardInterrupt()
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, interrupt)
    try:
        model = Model(out)
        stage = "fixture"
        # Import after setting lane ownership, before t1 creates any resources.
        os.environ["OO_TEST_LANE"] = "g"
        if args.publisher == "reviewed":
            from t6_reviewed import ReviewedGoldFixture as Fixture
        else:
            from t6_fixture import GoldFixture as Fixture
        fixture = Fixture(out, args.engine, args.scale)
        fixture.start()
        stage = "dialogue"
        prompt = bind_agent(model, fixture.cli, fixture.share_prompt(), out)
        for question in json.loads(QUESTIONS.read_text()):
            row = dialogue(model, fixture.cli, prompt, question,
                           fixture.gold[question["id"]], out)
            report["questions"].append({k: row[k] for k in (
                "id", "status", "equal", "null", "truncated", "refused", "error", "tool_failures")})
            write_json(out / "result.json", report)
        report["status"] = "passed" if all(q["equal"] for q in report["questions"]) else "failed"
    except (Exception, KeyboardInterrupt) as exc:
        report.update(stage=stage, error=type(exc).__name__, code=error_code(exc), trace=[
            {"file": Path(f.filename).name, "line": f.lineno, "function": f.name}
            for f in traceback.extract_tb(exc.__traceback__)])
    finally:
        if model:
            report["agent_model_calls"] = model.calls
            report["reserved_tokens"] = model.reserved
            model.close()
        builder_calls = 0 if args.publisher == "fixture" else None
        count_file = out / "builder-baseline/model-call-count.json"
        if count_file.exists():
            try:
                builder_calls = json.loads(count_file.read_text())["calls"]
            except (OSError, ValueError, KeyError):
                builder_calls = None
        report["builder_model_calls"] = builder_calls
        report["model_calls"] = ((model.calls if model else 0) + builder_calls
                                 if builder_calls is not None else None)
        try:
            if fixture:
                fixture.close()
            report["containers_left"] = 0
        except Exception as exc:
            report.update(status="failed", cleanup_error=type(exc).__name__)
        # Every question remains visible even when setup or credentials fail.
        answered = {q["id"] for q in report["questions"]}
        report["questions"].extend(
            dict(q, status="not_run", equal=False, null=None, truncated=None, refused=None)
            for q in json.loads(QUESTIONS.read_text())
            if q["id"] not in answered)
        report["status_counts"] = dict(Counter(q["status"] for q in report["questions"]))
        write_json(out / "result.json", report)
        checksums(out)
    print(json.dumps(report), flush=True)
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
