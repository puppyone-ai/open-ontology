"""HTTP surface of oo.doc-parser/v1 (10 §2.6). Standard library only."""
import hmac
import json
import re
import select
import socket
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from . import ADAPTER_VERSION, PROTOCOL
from .capabilities import table_structure_available
from .scratch import HEX64, Scratch, ScratchFull
from .supervisor import Pool
from .watchdog import Watchdog

FORMATS = ("pdf", "docx", "pptx")
BLOB_PATH = re.compile(r"^/v1/blobs/([0-9a-f]{64})$")


def load_models(path):
    try:
        lock = json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return []
    return [{"name": f["name"], "sha256": f["sha256"]} for f in lock.get("files", [])]


def parser_version():
    try:
        from importlib import metadata

        return metadata.version("docling-slim")
    except Exception:  # noqa: BLE001 - tests run without docling
        return "unavailable"


class Service:
    def __init__(self, cfg):
        self.cfg = cfg
        self.scratch = Scratch(cfg.scratch_dir, cfg.scratch_bytes)
        self.pool = Pool(cfg)
        self.models = load_models(cfg.models_lock)
        self.version = parser_version()
        self.watchdog = Watchdog(cfg.watchdog_s)

    def info(self):
        c = self.cfg
        return {
            "protocol": PROTOCOL,
            "parser_id": "docling",
            "parser_version": self.version,
            "adapter_version": ADAPTER_VERSION,
            "models": self.models,
            "formats": list(FORMATS),
            "workers": c.workers,
            "busy": self.pool.busy,
            "limits": {
                "max_blob_bytes": c.max_blob_bytes,
                "max_response_bytes": c.max_response_bytes,
                "max_deadline_ms": c.max_deadline_ms,
                "scratch_bytes": c.scratch_bytes,
                "ocr": False,
                "table_structure": table_structure_available(),
            },
            "stats": dict(self.pool.stats),
        }


class Server(ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 32

    def __init__(self, service):
        self.service = service
        super().__init__((service.cfg.host, service.cfg.port), Handler)

    def service_actions(self):
        self.service.watchdog.beat()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "oo-doc-parser"
    sys_version = ""

    # -- plumbing ---------------------------------------------------------
    def log_message(self, fmt, *args):  # no request lines with hashes in logs
        pass

    @property
    def svc(self):
        return self.server.service

    def _send(self, status, body=b"", ctype="application/json", close=False):
        self.send_response(status)
        if status != 204:
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
        else:
            self.send_header("Content-Length", "0")
        if close:
            self.send_header("Connection", "close")
            self.close_connection = True
        self.end_headers()
        if body and status != 204:
            self.wfile.write(body)

    def _json(self, status, obj, close=False):
        self._send(status, json.dumps(obj, separators=(",", ":")).encode(), close=close)

    def _error(self, status, code, message="", close=False, **extra):
        self._json(status, {"error": {"code": code, "message": message, **extra}}, close=close)

    def _authorized(self):
        got = self.headers.get("Authorization", "")
        want = "Bearer " + self.svc.cfg.token
        return hmac.compare_digest(got.encode(), want.encode())

    def _client_gone(self):
        try:
            r, _, _ = select.select([self.connection], [], [], 0)
            if r:
                return self.connection.recv(1, socket.MSG_PEEK) == b""
        except (OSError, ValueError):
            return True
        return False

    def _read_json(self, limit=64 * 1024):
        try:
            n = int(self.headers.get("Content-Length", ""))
        except ValueError:
            return None
        if n < 0 or n > limit:
            return None
        try:
            obj = json.loads(self.rfile.read(n) or b"null")
        except ValueError:
            return None
        return obj if isinstance(obj, dict) else None

    # -- routing ----------------------------------------------------------
    def do_GET(self):
        if self.path == "/healthz":
            return self._json(200, {"status": "ok"})
        if not self._authorized():
            return self._error(401, "PARSER_UNAUTHORIZED", "bearer token required", close=True)
        if self.path == "/v1/info":
            return self._json(200, self.svc.info())
        self._error(404, "PARSER_NOT_FOUND", "no such path")

    def do_PUT(self):
        if not self._authorized():
            return self._error(401, "PARSER_UNAUTHORIZED", "bearer token required", close=True)
        m = BLOB_PATH.match(self.path)
        if not m:
            return self._error(404, "PARSER_NOT_FOUND", "no such path", close=True)
        sha = m.group(1)
        try:
            size = int(self.headers.get("Content-Length", ""))
        except ValueError:
            return self._error(400, "PARSER_BAD_INPUT", "Content-Length is required", close=True)
        if size < 0 or size > self.svc.cfg.max_blob_bytes:
            return self._error(400, "PARSER_BAD_INPUT", "blob is larger than the limit", close=True)
        try:
            result, _ = self.svc.scratch.put(sha, size, self.rfile.read)
        except ScratchFull:
            return self._error(507, "PARSER_SCRATCH_FULL", "scratch is full", close=True)
        if result == "short":
            return self._error(400, "PARSER_BAD_INPUT", "body is shorter than Content-Length", close=True)
        if result == "mismatch":
            return self._error(409, "PARSER_HASH_MISMATCH", "body does not hash to the path", close=False)
        self._json(201 if result == "created" else 200, {"sha256": sha, "size": size})

    def do_DELETE(self):
        if not self._authorized():
            return self._error(401, "PARSER_UNAUTHORIZED", "bearer token required", close=True)
        m = BLOB_PATH.match(self.path)
        if not m:
            return self._error(404, "PARSER_NOT_FOUND", "no such path")
        self.svc.scratch.delete(m.group(1))
        self._send(204)

    def do_POST(self):
        if not self._authorized():
            return self._error(401, "PARSER_UNAUTHORIZED", "bearer token required", close=True)
        if self.path not in ("/v1/probe", "/v1/parse"):
            return self._error(404, "PARSER_NOT_FOUND", "no such path", close=True)
        req = self._read_json()
        if req is None:
            return self._error(400, "PARSER_BAD_INPUT", "body must be a small JSON object")
        sha, fmt = req.get("blob"), req.get("format")
        if not isinstance(sha, str) or not HEX64.match(sha):
            return self._error(400, "PARSER_BAD_INPUT", "blob must be a sha256 hex string")
        if fmt not in FORMATS or (self.path == "/v1/probe" and fmt != "pdf"):
            return self._error(422, "PARSER_UNSUPPORTED", "format is not supported here")
        if self.path == "/v1/probe":
            return self._run(sha, "probe", (fmt,), deadline_s=30)
        return self._parse(sha, fmt, req)

    def _parse(self, sha, fmt, req):
        cfg = self.svc.cfg
        rng = req.get("page_range")
        if rng is not None:
            ok = isinstance(rng, list) and len(rng) == 2 and all(isinstance(x, int) and not isinstance(x, bool) for x in rng)
            if not ok or rng[0] < 1 or rng[0] > rng[1] or fmt != "pdf":
                return self._error(400, "PARSER_BAD_INPUT", "page_range must be [start, end], 1-based, pdf only")
        opts = req.get("options") or {}
        if not isinstance(opts, dict):
            return self._error(400, "PARSER_BAD_INPUT", "options must be an object")
        if opts.get("ocr"):
            return self._error(422, "PARSER_UNSUPPORTED", "OCR is not available in this image (M2)")
        deadline_ms = req.get("deadline_ms", cfg.max_deadline_ms)
        if not isinstance(deadline_ms, int) or isinstance(deadline_ms, bool) or not 1 <= deadline_ms <= cfg.max_deadline_ms:
            return self._error(400, "PARSER_BAD_INPUT", f"deadline_ms must be 1..{cfg.max_deadline_ms}")
        table = bool(opts.get("table_structure", True))
        return self._run(sha, "parse", (fmt, rng, table, cfg.max_response_bytes), deadline_s=deadline_ms / 1000)

    def _run(self, sha, fn, extra, deadline_s):
        svc = self.svc
        if not svc.pool.try_acquire():
            return self._error(429, "PARSER_BUSY", "all workers are busy", retry_after_ms=2000)
        try:
            with svc.scratch.use(sha) as path:
                if path is None:
                    return self._error(404, "PARSER_BLOB_MISSING", "upload the blob first")
                out = svc.pool.run(fn, (str(path), *extra), deadline_s, self._client_gone)
        finally:
            svc.pool.release()
        k = out.kind
        if k == "ok":
            return self._send(200, out.body)
        if k == "cancelled":
            self.close_connection = True
            return
        status = {"timeout": 504, "crash": 500, "oom": 507}.get(k)
        if status is None:  # typed backend error
            status = {
                "PARSER_ENCRYPTED": 422,
                "PARSER_CORRUPT": 422,
                "PARSER_UNSUPPORTED": 422,
                "PARSER_OUTPUT_LIMIT": 422,
            }.get(out.code, 500)
        return self._error(status, out.code or "PARSER_CRASH", out.message)


def serve(cfg):
    svc = Service(cfg)
    srv = Server(svc)
    svc.watchdog.start()
    print(f"doc-parser listening on {cfg.host}:{srv.server_address[1]} workers={cfg.workers}", file=sys.stderr, flush=True)
    try:
        srv.serve_forever(poll_interval=0.5)
    finally:
        svc.watchdog.stop()
        srv.server_close()
