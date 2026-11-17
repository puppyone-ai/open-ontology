"""One child process per request (10 §2.6), supervised from a handler thread.

The child gets a pipe, runs `backend.<fn>(*args)` and sends back either
`\\x00 + response body` or `\\x01 + {"code","message"}`. The parent turns every way a
child can end into a typed outcome: ok, error, timeout (we killed it at the
deadline), crash (signal or non-zero exit without an answer), oom (SIGKILL and
the cgroup oom_kill counter moved), cancelled (the client went away).
"""
import importlib
import json
import multiprocessing
import os
import signal
import threading
import time
from dataclasses import dataclass
from pathlib import Path


class WorkerError(Exception):
    """Raised by a backend to report a typed failure."""

    def __init__(self, code, message=""):
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message


@dataclass
class Outcome:
    kind: str  # ok | error | timeout | crash | oom | cancelled
    body: bytes = b""
    code: str = ""
    message: str = ""


def _child_main(conn, backend, fn, args):
    try:
        try:
            import resource

            resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        except (ImportError, ValueError, OSError):
            pass
        mod = importlib.import_module(backend)
        body = getattr(mod, fn)(*args)
        conn.send_bytes(b"\x00" + body)
    except WorkerError as e:
        conn.send_bytes(b"\x01" + json.dumps({"code": e.code, "message": e.message}).encode())
    except BaseException as e:  # noqa: BLE001 - a worker must always answer
        msg = f"worker error: {type(e).__name__}"
        conn.send_bytes(b"\x01" + json.dumps({"code": "PARSER_CRASH", "message": msg}).encode())
    finally:
        conn.close()


def read_oom_kills(path):
    try:
        for line in Path(path).read_text().splitlines():
            key, _, value = line.partition(" ")
            if key == "oom_kill":
                return int(value)
    except (OSError, ValueError):
        pass
    return 0


class Pool:
    def __init__(self, cfg):
        self.cfg = cfg
        self.ctx = multiprocessing.get_context(cfg.start_method)
        if cfg.start_method == "forkserver":
            self.ctx.set_forkserver_preload(list(cfg.preload))
        self._slots = threading.BoundedSemaphore(cfg.workers)
        self._lock = threading.Lock()
        self.busy = 0
        self.stats = {"ok": 0, "error": 0, "timeout": 0, "crash": 0, "oom": 0, "cancelled": 0, "busy_rejected": 0}

    def try_acquire(self):
        if not self._slots.acquire(blocking=False):
            with self._lock:
                self.stats["busy_rejected"] += 1
            return False
        with self._lock:
            self.busy += 1
        return True

    def release(self):
        with self._lock:
            self.busy -= 1
        self._slots.release()

    def _count(self, kind):
        with self._lock:
            self.stats[kind] += 1

    def run(self, fn, args, deadline_s, is_cancelled=lambda: False):
        """Run one job. The caller holds a slot (try_acquire/release)."""
        oom_before = read_oom_kills(self.cfg.cgroup_events)
        recv, send = self.ctx.Pipe(duplex=False)
        proc = self.ctx.Process(target=_child_main, args=(send, self.cfg.backend, fn, args), daemon=True)
        proc.start()
        send.close()
        deadline = time.monotonic() + deadline_s
        we_killed = None
        out = None
        try:
            while True:
                if recv.poll(0.05):
                    try:
                        data = recv.recv_bytes()
                    except (EOFError, OSError):
                        break  # child died without an answer
                    out = data
                    break
                if not proc.is_alive():
                    # Drain a message that raced with exit.
                    if recv.poll(0.2):
                        try:
                            out = recv.recv_bytes()
                        except (EOFError, OSError):
                            pass
                    break
                if time.monotonic() >= deadline:
                    we_killed = "timeout"
                    break
                if is_cancelled():
                    we_killed = "cancelled"
                    break
        finally:
            if we_killed or proc.is_alive() and out is None:
                proc.kill()
            proc.join(5)
            if proc.is_alive():
                proc.kill()
                proc.join(5)
            recv.close()
        if we_killed:
            self._count(we_killed)
            return Outcome(we_killed, code="PARSER_TIMEOUT" if we_killed == "timeout" else "")
        if out is not None:
            if out[:1] == b"\x00":
                self._count("ok")
                return Outcome("ok", body=out[1:])
            info = json.loads(out[1:] or b"{}")
            self._count("error")
            return Outcome("error", code=info.get("code", "PARSER_CRASH"), message=info.get("message", ""))
        code = proc.exitcode
        if code == -signal.SIGKILL and read_oom_kills(self.cfg.cgroup_events) > oom_before:
            self._count("oom")
            return Outcome("oom", code="PARSER_OOM", message="worker was killed by the memory limit")
        self._count("crash")
        how = f"signal {-code}" if code is not None and code < 0 else f"exit {code}"
        return Outcome("crash", code="PARSER_CRASH", message=f"worker died ({how})")
