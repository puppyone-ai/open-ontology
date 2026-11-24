#!/usr/bin/env python3
"""Deterministic OpenAI-compatible model for test engines (Desktop T1 states, lane I).

It is a model, not a fake engine: the engine sends its real `extract/2` request through the real egress process and
parses, checks and publishes whatever this server answers. The answer is a pure function of the request and of an
optional control file, so a run is repeatable.

Documents tell it what to extract with markers in the text (the marker itself is the verbatim quote):

  [[Person: 林川]]  [[Project: 星图]]            entity of a core type
  [[Thing/合同: 合同001]]                          open `Thing` with the label 合同
  [[owns: 林川 > 星图]]                             relation (subject name > object name; both must be markers)

Control file (`--control FILE`, JSON, read again on every request): `{"skip": ["林川", ...]}` leaves the named
entities (and the relations that use them) out of the answer, which is how a second extraction gets a different
result than the first one without touching the document.

  fixture_model.py --port 0 [--control FILE] [--port-file FILE]

Prints `{"port": N}` on the first stdout line. Standard library only. Loopback only.
"""
import argparse
import json
import re
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

MARK = re.compile(r"\[\[([^\[\]:]+?)(?:/([^\[\]:]+?))?: ([^\[\]]+?)\]\]")
REL = re.compile(r"\[\[(owns|member_of|decides|about|rejects|supersedes|mentions|tried_and_failed): ([^\[\]>]+?) > ([^\[\]]+?)\]\]")
TYPES = {"Person", "Organization", "Project", "Decision", "Topic", "Event", "Term", "Thing"}


def blocks_of(user):
    out = []
    inside = False
    for line in user.splitlines():
        if line.startswith("<<<BLOCKS"):
            inside = True
        elif line.startswith("BLOCKS>>>"):
            inside = False
        elif inside and line.strip():
            try:
                out.append(json.loads(line))
            except ValueError:
                pass
    return out


def answer(user, skip):
    entities, relations, ids = [], [], {}
    for b in blocks_of(user):
        text = b.get("text", "")
        for m in MARK.finditer(text):
            kind, label, name = m.group(1), m.group(2), m.group(3).strip()
            if kind not in TYPES or name in skip or name in ids:
                continue
            ids[name] = f"e{len(ids) + 1}"
            e = {"id": ids[name], "type": kind, "name": name, "aliases": [], "block_id": b["block_id"],
                 "quote": m.group(0), "confidence": "high"}
            if kind == "Thing":
                e["label"] = label or "物件"
            entities.append(e)
    for b in blocks_of(user):
        for m in REL.finditer(b.get("text", "")):
            s, o = m.group(2).strip(), m.group(3).strip()
            if s in ids and o in ids:
                relations.append({"subject": ids[s], "predicate": m.group(1), "object": ids[o],
                                  "block_id": b["block_id"], "quote": m.group(0), "confidence": "high"})
    return {"entities": entities, "relations": relations}


def make_handler(control):
    class H(BaseHTTPRequestHandler):
        def log_message(self, *a):  # no access log
            pass

        def _send(self, code, obj):
            body = json.dumps(obj, ensure_ascii=False).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path.rstrip("/").endswith("/models"):
                self._send(200, {"object": "list", "data": [{"id": "fixture-model", "object": "model"}]})
            else:
                self._send(404, {"error": "not found"})

        def do_POST(self):
            if not self.path.endswith("/chat/completions"):
                return self._send(404, {"error": "not found"})
            req = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))) or b"{}")
            skip = set()
            if control and Path(control).is_file():
                try:
                    skip = set(json.loads(Path(control).read_text()).get("skip", []))
                except ValueError:
                    pass
            user = next((m["content"] for m in reversed(req.get("messages", [])) if m.get("role") == "user"), "")
            content = json.dumps(answer(user, skip), ensure_ascii=False)
            self._send(200, {"id": "fixture", "object": "chat.completion", "model": req.get("model", "fixture-model"),
                             "choices": [{"index": 0, "message": {"role": "assistant", "content": content},
                                          "finish_reason": "stop"}],
                             "usage": {"prompt_tokens": max(1, len(user) // 4), "completion_tokens": max(1, len(content) // 4),
                                       "total_tokens": max(2, (len(user) + len(content)) // 4)}})

    return H


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--port", type=int, default=0)
    ap.add_argument("--control")
    ap.add_argument("--port-file")
    a = ap.parse_args()
    srv = ThreadingHTTPServer(("127.0.0.1", a.port), make_handler(a.control))
    port = srv.server_address[1]
    if a.port_file:
        Path(a.port_file).write_text(str(port))
    print(json.dumps({"port": port}), flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        sys.exit(0)


if __name__ == "__main__":
    main()
