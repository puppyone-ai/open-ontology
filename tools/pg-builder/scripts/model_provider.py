"""Provider authentication and bounded readiness probes; no credential persistence."""
import argparse
import http.client
import json
import os
from pathlib import Path
import re
import time
import urllib.error
import urllib.request
from urllib.parse import urlsplit


def credential(config):
    if "api_key_env" in config:
        name = config["api_key_env"]
        if not isinstance(name, str) or not re.fullmatch(r"[A-Z][A-Z0-9_]{0,63}", name):
            raise ValueError("model_key_env_invalid")
        value = os.environ.get(name, "").strip()
    else:
        value = Path(config["bearer_file"]).read_text().strip()
    if not value or "\n" in value or "\r" in value:
        raise ValueError("model_credential_missing_or_invalid")
    return value


def endpoint(config):
    value = config["endpoint"].rstrip("/")
    parsed = urlsplit(value)
    if (parsed.username or parsed.password or parsed.query or parsed.fragment or not parsed.hostname
            or parsed.scheme != "https" and not (
                parsed.scheme == "http" and parsed.hostname in {"localhost", "127.0.0.1", "::1"})):
        raise ValueError("model_endpoint_requires_https_or_loopback")
    return value


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def open_request(request, timeout):
    # Never forward a bearer to an unexpected redirected endpoint.
    return urllib.request.build_opener(NoRedirect()).open(request, timeout=timeout)


class ProviderFailure(RuntimeError):
    def __init__(self, code, retryable, stops_run=False):
        super().__init__(code)
        self.retryable = retryable
        self.stops_run = stops_run

def http_failure(status, body):
    """Only fixed provider tokens escape; raw errors remain in private evidence."""
    try:
        value = json.loads(body)
        error = value.get('error', {}) if isinstance(value, dict) else {}
        tokens = [error.get('type'), error.get('code')] if isinstance(error, dict) else []
    except (ValueError, TypeError):
        tokens = []
    for token in ('usage_limit_reached', 'insufficient_quota', 'model_cooldown', 'cooldown'):
        if token in tokens:
            return ProviderFailure('s5_provider_' + token, False, True)
    if status == 429:
        return ProviderFailure('s5_provider_http_429', False, True)
    return ProviderFailure('s5_provider_http_' + str(status),
                           status in (408, 429) or status >= 500, status in (401, 403))

class CompletionStreamError(ValueError):
    def __init__(self, code, raw):
        super().__init__(code)
        self.raw_response = bytes(raw)

def read_completion_stream(response, max_bytes=8_388_608, timeout=600):
    """Assemble one text-only SSE completion; incomplete streams are not drafts."""
    started, raw, content = time.monotonic(), bytearray(), []
    event, finish, usage, model, done = [], None, None, None, False
    while True:
        try:
            line = response.readline(min(max_bytes - len(raw) + 1, 1_048_577))
        except (OSError, TimeoutError, http.client.HTTPException):
            raise CompletionStreamError('s5_stream_transport_failed', raw) from None
        if not line:
            break
        raw.extend(line)
        if len(raw) > max_bytes or len(line) > 1_048_576:
            raise CompletionStreamError('s5_response_budget', raw)
        if time.monotonic() - started > timeout:
            raise CompletionStreamError('s5_stream_deadline', raw)
        line = line.rstrip(b'\r\n')
        if line.startswith(b'data:'):
            event.append(line[5:].lstrip(b' '))
        elif not line and event:
            data = b'\n'.join(event)
            event = []
            if data == b'[DONE]':
                done = True
                break
            try:
                payload = json.loads(data)
                if isinstance(payload, dict) and payload.get('error'):
                    failure = http_failure(400, data)
                    if not failure.retryable and failure.stops_run:
                        refused = CompletionStreamError(str(failure), raw)
                        refused.retryable = False
                        refused.stops_run = True
                        raise refused
                if not isinstance(payload, dict) or payload.get('error'):
                    raise ValueError
                choices = payload.get('choices', [])
                if not isinstance(choices, list) or len(choices) > 1:
                    raise ValueError
                model = payload.get('model', model)
                if payload.get('usage') is not None:
                    usage = payload['usage']
                for choice in choices:
                    if choice.get('index', 0) != 0:
                        raise ValueError
                    delta = choice.get('delta', {})
                    if not isinstance(delta, dict) or delta.get('tool_calls') or delta.get('function_call'):
                        raise ValueError
                    text = delta.get('content')
                    if text is not None:
                        if not isinstance(text, str) or finish is not None and text:
                            raise ValueError
                        content.append(text)
                    if choice.get('finish_reason') is not None:
                        if finish is not None and finish != choice['finish_reason']:
                            raise ValueError
                        finish = choice['finish_reason']
            except CompletionStreamError:
                raise
            except (ValueError, KeyError, TypeError, AttributeError):
                raise CompletionStreamError('s5_stream_invalid', raw) from None
    # Some compatible providers close cleanly after the terminal finish frame.
    # EOF without that frame (or inside an unfinished event) remains a failure.
    if not finish or event:
        raise CompletionStreamError('s5_stream_incomplete', raw)
    return {'model': model, 'choices': [{'finish_reason': finish,
            'message': {'content': ''.join(content)}}], 'usage': usage,
            'stream_terminal': 'done' if done else 'finish_frame_eof'}, bytes(raw)

def probe(config):
    require_listing = config.get("require_model_listing", True)
    if type(require_listing) is not bool:
        raise ValueError("model_listing_probe_config")
    timeout = config.get("request_timeout_seconds", 45)
    if type(timeout) is not int or not 1 <= timeout <= 600:
        raise ValueError("model_probe_timeout")
    tools_required = config.get("require_tools", False)
    if type(tools_required) is not bool:
        raise ValueError("model_tool_probe_config")
    base, key = endpoint(config), credential(config)
    report = {"protocol": "oo.model-readiness/v1", "model": config["model"],
              "ready": False, "checks": []}
    calls = [
        ("/models", None, "listing"),
        ("/chat/completions", {"model": config["model"],
                              "messages": [{"role": "user", "content": "Reply exactly OK."}],
                              "max_tokens": 1024, "stream": False}, "generation"),
    ]
    tool_request = {
        "model": config["model"], "max_tokens": 1024, "stream": False,
        "messages": [{"role": "user", "content":
                      "Call read_acceptance_fixture with fixture_id readiness, then reply with its value only."}],
        "tools": [{"type": "function", "function": {
            "name": "read_acceptance_fixture", "description": "Read a synthetic readiness fixture.",
            "parameters": {"type": "object", "properties": {
                "fixture_id": {"type": "string", "enum": ["readiness"]}},
                "required": ["fixture_id"], "additionalProperties": False}}}],
    }
    for path, body, stage in calls:
        request = urllib.request.Request(
            base + path, data=json.dumps(body).encode() if body is not None else None,
            headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"})
        check = {"path": path, "stage": stage, "ok": False}
        started = time.monotonic()
        try:
            with open_request(request, timeout=timeout) as response:
                raw = response.read(1_048_577)
            if len(raw) > 1_048_576:
                raise ValueError("model_response_budget")
            value = json.loads(raw)
            if body is None:
                check["ok"] = any(m.get("id") == config["model"] for m in value["data"])
                check["code"] = "listed" if check["ok"] else "model_not_listed"
            elif stage == "tool_call":
                choice = value["choices"][0]
                tools = choice["message"].get("tool_calls", [])
                check["finish_reason"] = (choice.get("finish_reason") if choice.get("finish_reason")
                                          in {"stop", "tool_calls", "length", "content_filter"} else "other")
                check["tool_count"] = len(tools)
                check["content_present"] = bool(choice["message"].get("content"))
                valid = choice.get("finish_reason") == "tool_calls" and len(tools) == 1
                tool = tools[0] if valid else {}
                function = tool.get("function", {})
                check["expected_function"] = function.get("name") == "read_acceptance_fixture"
                check["valid_arguments"] = json.loads(function.get("arguments", "null")) == {"fixture_id": "readiness"}
                check["valid_call_id"] = isinstance(tool.get("id"), str) and 0 < len(tool["id"]) <= 256
                valid = (valid and tool.get("type") == "function"
                         and check["valid_call_id"] and check["expected_function"]
                         and check["valid_arguments"])
                check.update(ok=bool(valid), code="tool_call_verified" if valid else "tool_call_unverified")
                if valid:
                    followup = dict(tool_request, messages=tool_request["messages"] + [
                        {"role": "assistant", "content": None, "tool_calls": [tool]},
                        {"role": "tool", "tool_call_id": tool["id"], "content": '{"value":"READY42"}'},
                    ])
                    calls.append(("/chat/completions", followup, "tool_result"))
            else:
                choice = value["choices"][0]
                expected = "READY42" if stage == "tool_result" else "OK"
                check["finish_reason"] = (choice.get("finish_reason") if choice.get("finish_reason")
                                          in {"stop", "tool_calls", "length", "content_filter"} else "other")
                check["ok"] = choice.get("finish_reason") == "stop" and choice["message"]["content"].strip() == expected
                check["code"] = stage + ("_verified" if check["ok"] else "_unverified")
                if stage == "generation" and check["ok"] and tools_required:
                    calls.append(("/chat/completions", tool_request, "tool_call"))
        except urllib.error.HTTPError as error:
            check.update(http_status=error.code, code="provider_http_error")
            try:
                code = json.loads(error.read(4096)).get("error", {}).get("code")
                if code in {"IP_BLOCKED", "invalid_api_key", "insufficient_quota", "rate_limit_exceeded"}:
                    check["code"] = code
            except (ValueError, AttributeError):
                pass
        except (urllib.error.URLError, OSError, TimeoutError, http.client.HTTPException) as error:
            check["code"] = ("provider_timeout" if isinstance(error, TimeoutError)
                             or isinstance(getattr(error, "reason", None), TimeoutError)
                             else "provider_transport_failed")
        except (ValueError, KeyError, TypeError, AttributeError, IndexError):
            check["code"] = "provider_response_invalid"
        check["elapsed_ms"] = int((time.monotonic() - started) * 1000)
        report["checks"].append(check)
        listing_optional = (stage == "listing" and not require_listing
                            and (check.get("code") == "model_not_listed"
                                 or check.get("http_status") in {404, 405}))
        if not check["ok"] and not listing_optional:
            break
    report["model_listing_required"] = require_listing
    report["ready"] = len(report["checks"]) == (4 if tools_required else 2) and all(
        c["ok"] or c["stage"] == "listing" and not require_listing
        and (c.get("code") == "model_not_listed" or c.get("http_status") in {404, 405})
        for c in report["checks"])
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, type=Path)
    args = parser.parse_args()
    try:
        report = probe(json.loads(args.config.read_text()))
    except (ValueError, KeyError, OSError):
        report = {"protocol": "oo.model-readiness/v1", "ready": False, "code": "provider_configuration_invalid"}
    print(json.dumps(report))
    return 0 if report["ready"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
