import hashlib
import http.client
import json
import sys
import threading
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "tests"))

from doc_parser.config import Config  # noqa: E402
from doc_parser.server import Server, Service  # noqa: E402

TOKEN = "test-token"


def sha(b):
    return hashlib.sha256(b).hexdigest()


class Client:
    def __init__(self, port):
        self.port = port

    def call(self, method, path, body=None, headers=None, token=TOKEN, raw=False):
        h = {} if token is None else {"Authorization": f"Bearer {token}"}
        h.update(headers or {})
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=30)
        try:
            c.request(method, path, body=body, headers=h)
            r = c.getresponse()
            data = r.read()
            return r.status, (data if raw else (json.loads(data) if data else None))
        finally:
            c.close()

    def put(self, data, digest=None, **kw):
        return self.call("PUT", f"/v1/blobs/{digest or sha(data)}", body=data, **kw)

    def post(self, path, obj, **kw):
        return self.call("POST", path, body=json.dumps(obj).encode(), headers={"Content-Type": "application/json"}, **kw)


@pytest.fixture
def make_service(tmp_path, monkeypatch):
    started = []

    def make(**over):
        events = tmp_path / "memory.events"
        events.write_text("oom_kill 0\n")
        monkeypatch.setenv("OO_PARSER_CGROUP_EVENTS", str(events))
        cfg = dict(
            token=TOKEN, host="127.0.0.1", port=0, workers=2, scratch_dir=str(tmp_path / "scratch"),
            backend="fake_backend", start_method="fork", models_lock=str(tmp_path / "none.json"),
            cgroup_events=str(events), watchdog_s=3600,
        )
        cfg.update(over)
        svc = Service(Config(**cfg))
        srv = Server(svc)
        t = threading.Thread(target=srv.serve_forever, kwargs={"poll_interval": 0.05}, daemon=True)
        t.start()
        started.append(srv)
        return svc, Client(srv.server_address[1]), srv

    yield make
    for srv in started:
        srv.shutdown()
        srv.server_close()
