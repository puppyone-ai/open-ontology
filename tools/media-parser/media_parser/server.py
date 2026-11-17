"""HTTP surface of media-parser: `oo.doc-parser/v1` plus long tasks (10 section 17.1).

`POST /v1/parse` answers `202 {"task": id}` as soon as the job holds a worker slot; `GET /v1/tasks/{id}` gives
`{state: running|done|failed|cancelled, done_ms, total_ms}` and, at the end, `result` (the parse response) or `error`;
`DELETE /v1/tasks/{id}` kills the worker process. `POST /v1/probe` is synchronous (ffprobe plus the codec verdict).
Everything else (auth, blobs, scratch, errors) is the `doc_parser` server.
"""
import json
import re
import threading
import uuid

from doc_parser.scratch import HEX64
from doc_parser.server import BLOB_PATH, Handler as BaseHandler
from doc_parser.server import Server as BaseServer
from doc_parser.server import Service as BaseService

from . import errors
from .meta import MAX_DURATION_MS

TASK_PATH = re.compile(r"^/v1/tasks/([0-9a-f]{32})$")
ERROR_STATUS = {
    errors.CODEC_UNAVAILABLE: 422, errors.MEDIA_TOO_LONG: 422, errors.HOST_DECODE_MISMATCH: 422,
    errors.ASR_FAILED: 500, errors.DIARIZATION_FAILED: 500, errors.DISK_LOW: 507,
    "PARSER_ENCRYPTED": 422, "PARSER_CORRUPT": 422, "PARSER_UNSUPPORTED": 422, "PARSER_OUTPUT_LIMIT": 422,
}


class Task:
    def __init__(self, sha, fmt):
        self.id = uuid.uuid4().hex
        self.sha, self.fmt = sha, fmt
        self.state = "running"
        self.cancel = threading.Event()
        self.result = None
        self.error = None


class Service(BaseService):
    def __init__(self, cfg):
        super().__init__(cfg)
        self.tasks = {}
        self.tasks_lock = threading.Lock()

    def job_dir(self, sha):
        d = self.scratch.root / "media-jobs" / sha
        d.mkdir(parents=True, exist_ok=True)
        return d

    def progress(self, sha):
        try:
            return json.loads((self.scratch.root / "media-jobs" / sha / "progress.json").read_text())
        except (OSError, ValueError):
            return {}


class Handler(BaseHandler):
    server_version = "oo-media-parser"

    def _task_json(self, t):
        body = {"id": t.id, "state": t.state, **{k: v for k, v in self.svc.progress(t.sha).items() if k in ("done_ms", "total_ms")}}
        if t.state == "done":
            body["result"] = t.result
        if t.state == "failed":
            body["error"] = t.error
        return body

    def do_GET(self):
        m = TASK_PATH.match(self.path)
        if not m:
            return super().do_GET()
        if not self._authorized():
            return self._error(401, "PARSER_UNAUTHORIZED", "bearer token required", close=True)
        t = self.svc.tasks.get(m.group(1))
        if t is None:
            return self._error(404, "PARSER_NOT_FOUND", "no such task")
        self._json(200, self._task_json(t))

    def do_DELETE(self):
        m = TASK_PATH.match(self.path)
        if not m:
            return super().do_DELETE()
        if not self._authorized():
            return self._error(401, "PARSER_UNAUTHORIZED", "bearer token required", close=True)
        t = self.svc.tasks.get(m.group(1))
        if t is None:
            return self._error(404, "PARSER_NOT_FOUND", "no such task")
        t.cancel.set()
        self._send(204)

    def do_POST(self):
        if self.path != "/v1/parse":
            return super().do_POST()
        if not self._authorized():
            return self._error(401, "PARSER_UNAUTHORIZED", "bearer token required", close=True)
        req = self._read_json()
        if req is None:
            return self._error(400, "PARSER_BAD_INPUT", "body must be a small JSON object")
        sha, fmt = req.get("blob"), req.get("format")
        if not isinstance(sha, str) or not HEX64.match(sha):
            return self._error(400, "PARSER_BAD_INPUT", "blob must be a sha256 hex string")
        if fmt not in self.svc.meta.FORMATS:
            return self._error(422, "PARSER_UNSUPPORTED", "format is not supported here")
        opts = req.get("options") or {}
        if not isinstance(opts, dict):
            return self._error(400, "PARSER_BAD_INPUT", "options must be an object")
        if not self.svc.scratch.has(sha):
            return self._error(404, "PARSER_BLOB_MISSING", "upload the blob first")
        if not self.svc.pool.try_acquire():
            return self._error(429, "PARSER_BUSY", "all workers are busy", retry_after_ms=2000)
        task = Task(sha, fmt)
        with self.svc.tasks_lock:
            self.svc.tasks[task.id] = task
        threading.Thread(target=self._run_task, args=(self.svc, task, opts), daemon=True).start()
        self._json(202, {"task": task.id})

    @staticmethod
    def _run_task(svc, task, opts):
        cfg = svc.cfg
        try:
            with svc.scratch.use(task.sha) as path:
                if path is None:
                    task.state, task.error = "failed", {"code": "PARSER_BLOB_MISSING", "message": "blob vanished"}
                    return
                job = svc.job_dir(task.sha)
                deadline_s = max(60.0, MAX_DURATION_MS / 1000 * 2)
                out = svc.pool.run("parse_task", (str(path), task.fmt, opts, cfg.max_response_bytes, str(job), task.sha),
                                   deadline_s, task.cancel.is_set)
            if out.kind == "ok":
                task.result, task.state = json.loads(out.body), "done"
            elif out.kind == "cancelled" or task.cancel.is_set():
                task.state = "cancelled"
            else:
                message, details = errors.split_message(out.message)
                code = out.code or "PARSER_CRASH"
                task.error = {"code": code, "message": message, "status": {"timeout": 504, "crash": 500, "oom": 507}.get(out.kind, ERROR_STATUS.get(code, 500))}
                if details is not None:
                    task.error["details"] = details
                task.state = "failed"
        except Exception as e:  # noqa: BLE001 - a task always ends in a state
            task.state, task.error = "failed", {"code": "PARSER_CRASH", "message": type(e).__name__, "status": 500}
        finally:
            svc.pool.release()


class Server(BaseServer):
    def __init__(self, service):
        super().__init__(service)
        self.RequestHandlerClass = Handler


def serve(cfg):
    import sys

    svc = Service(cfg)
    srv = Server(svc)
    svc.watchdog.start()
    print(f"media sidecar listening on {cfg.host}:{srv.server_address[1]} workers={cfg.workers}", file=sys.stderr, flush=True)
    try:
        srv.serve_forever(poll_interval=0.5)
    finally:
        svc.watchdog.stop()
        srv.server_close()
