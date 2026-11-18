import json
import threading
import time

import pytest
from conftest import TOKEN, sha

import media_parser.server as ms
from doc_parser.config import Config

from conftest import _dp  # noqa: F401


@pytest.fixture
def media(tmp_path, monkeypatch):
    started = []

    def make(workers=1, **over):
        events = tmp_path / "memory.events"
        events.write_text("oom_kill 0\n")
        cfg = dict(token=TOKEN, host="127.0.0.1", port=0, workers=workers, scratch_dir=str(tmp_path / "scratch"),
                   backend="fake_media_backend", meta="media_parser.meta", start_method="fork",
                   models_lock=str(tmp_path / "none.json"), cgroup_events=str(events), watchdog_s=3600)
        cfg.update(over)
        svc = ms.Service(Config(**cfg))
        srv = ms.Server(svc)
        threading.Thread(target=srv.serve_forever, kwargs={"poll_interval": 0.05}, daemon=True).start()
        started.append(srv)
        return svc, _dp.Client(srv.server_address[1])

    yield make
    for s in started:
        s.shutdown()
        s.server_close()


def wait(c, task, states=("done", "failed", "cancelled"), timeout=10):
    t0 = time.time()
    while time.time() - t0 < timeout:
        st, body = c.call("GET", f"/v1/tasks/{task}")
        assert st == 200
        if body["state"] in states:
            return body
        time.sleep(0.05)
    raise AssertionError("task did not finish")


def test_info_names_the_media_parser(media):
    _, c = media()
    st, info = c.call("GET", "/v1/info")
    assert st == 200 and info["parser_id"] == "media" and info["formats"] == ["audio", "video"] and info["workers"] == 1
    assert info["limits"]["max_media_bytes"] == 4 << 30 and info["limits"]["long_tasks"] is True
    spdx = {x["name"]: x["spdx"] for x in info["licenses"] if "spdx" in x}
    assert spdx["ffmpeg"] == "LGPL-2.1-or-later" and "qwen-asr" not in spdx and "pyannote.audio" not in spdx


def test_parse_is_a_task_with_progress_and_a_result(media):
    svc, c = media()
    blob = b"SLOW-audio"
    c.put(blob)
    st, body = c.post("/v1/parse", {"blob": sha(blob), "format": "audio", "options": {"hotwords": ["灯塔"]}})
    assert st == 202 and len(body["task"]) == 32
    done = wait(c, body["task"])
    assert done["state"] == "done" and done["result"]["kind"] == "audio" and done["result"]["opts"] == {"hotwords": ["灯塔"]}
    assert done["done_ms"] == 500 and done["total_ms"] == 1000
    assert svc.pool.busy == 0, "the worker slot is free after the task"


def test_only_one_media_job_at_a_time_by_default_and_busy_is_429(media):
    _, c = media(workers=1)
    blob = b"SLEEP-audio"
    c.put(blob)
    st, first = c.post("/v1/parse", {"blob": sha(blob), "format": "audio"})
    assert st == 202
    st, body = c.post("/v1/parse", {"blob": sha(blob), "format": "audio"})
    assert st == 429 and body["error"]["code"] == "PARSER_BUSY"
    c.call("DELETE", f"/v1/tasks/{first['task']}")
    assert wait(c, first["task"])["state"] == "cancelled"


def test_cancel_kills_the_worker(media):
    svc, c = media()
    blob = b"SLEEP-x"
    c.put(blob)
    task = c.post("/v1/parse", {"blob": sha(blob), "format": "video"})[1]["task"]
    time.sleep(0.5)
    st, mid = c.call("GET", f"/v1/tasks/{task}")
    assert mid["state"] == "running" and mid["total_ms"] == 60000 and mid["done_ms"] > 0
    assert c.call("DELETE", f"/v1/tasks/{task}")[0] == 204
    assert wait(c, task)["state"] == "cancelled"
    deadline = time.time() + 5
    while svc.pool.busy and time.time() < deadline:
        time.sleep(0.05)
    assert svc.pool.busy == 0 and svc.pool.stats["cancelled"] == 1


def test_typed_errors_carry_details(media):
    _, c = media()
    for blob, code, key in ((b"CODEC-1", "PARSER_CODEC_UNAVAILABLE", "how_to"), (b"LONG-1", "PARSER_MEDIA_TOO_LONG", "duration_ms")):
        c.put(blob)
        task = c.post("/v1/parse", {"blob": sha(blob), "format": "video"})[1]["task"]
        done = wait(c, task)
        assert done["state"] == "failed" and done["error"]["code"] == code and key in done["error"]["details"]
        assert done["error"]["status"] == 422


def test_input_checks_and_auth(media):
    _, c = media()
    assert c.post("/v1/parse", {"blob": "a" * 64, "format": "audio"})[0] == 404
    assert c.post("/v1/parse", {"blob": "a" * 64, "format": "pdf"})[0] == 422
    assert c.post("/v1/parse", {"blob": "zz", "format": "audio"})[0] == 400
    assert c.post("/v1/parse", {"blob": "a" * 64, "format": "audio"}, token="bad")[0] == 401
    assert c.call("GET", "/v1/tasks/" + "0" * 32)[0] == 404
    assert c.call("GET", "/v1/tasks/" + "0" * 32, token=None)[0] == 401
    blob = b"ok-audio"
    c.put(blob)
    st, body = c.post("/v1/probe", {"blob": sha(blob), "format": "audio"})
    assert st == 200 and body["decode"] == "builtin"
