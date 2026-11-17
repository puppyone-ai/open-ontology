"""Environment configuration. Everything has a default that matches 10 §2.6/§2.7."""
import os
from dataclasses import dataclass
from pathlib import Path

GIB = 1 << 30
MIB = 1 << 20


@dataclass(frozen=True)
class Config:
    token: str
    host: str = "0.0.0.0"
    port: int = 7481
    workers: int = 2
    scratch_dir: str = "/scratch"
    scratch_bytes: int = 2 * GIB
    max_blob_bytes: int = 209_715_200
    max_response_bytes: int = 64 * MIB
    max_deadline_ms: int = 120_000
    watchdog_s: float = 60.0
    backend: str = "doc_parser.adapter"
    meta: str = "doc_parser.meta"
    start_method: str = "forkserver"
    models_lock: str = "/opt/app/models.lock.json"
    cgroup_events: str = "/sys/fs/cgroup/memory.events"
    # Models are imported (not loaded) once in the fork server so each worker
    # starts warm.
    preload: tuple = ("doc_parser.adapter",)

    @classmethod
    def from_env(cls, env=None):
        env = os.environ if env is None else env
        token = ""
        path = env.get("OO_PARSER_TOKEN_FILE")
        if path:
            token = Path(path).read_text().strip()
        token = token or env.get("OO_PARSER_TOKEN", "").strip()
        if not token:
            raise SystemExit("doc-parser: no bearer token (OO_PARSER_TOKEN_FILE or OO_PARSER_TOKEN)")

        def num(name, default, cast=int):
            return cast(env[name]) if name in env else default

        d = cls.__dataclass_fields__
        return cls(
            token=token,
            host=env.get("OO_PARSER_HOST", d["host"].default),
            port=num("OO_PARSER_PORT", d["port"].default),
            workers=num("OO_PARSER_WORKERS", d["workers"].default),
            scratch_dir=env.get("OO_PARSER_SCRATCH", d["scratch_dir"].default),
            scratch_bytes=num("OO_PARSER_SCRATCH_BYTES", d["scratch_bytes"].default),
            max_blob_bytes=num("OO_PARSER_MAX_BLOB_BYTES", d["max_blob_bytes"].default),
            max_response_bytes=num("OO_PARSER_MAX_RESPONSE_BYTES", d["max_response_bytes"].default),
            max_deadline_ms=num("OO_PARSER_MAX_DEADLINE_MS", d["max_deadline_ms"].default),
            watchdog_s=num("OO_PARSER_WATCHDOG_S", d["watchdog_s"].default, float),
            backend=env.get("OO_PARSER_BACKEND", d["backend"].default),
            meta=env.get("OO_PARSER_META", d["meta"].default),
            start_method=env.get("OO_PARSER_START_METHOD", d["start_method"].default),
            models_lock=env.get("OO_PARSER_MODELS_LOCK", d["models_lock"].default),
            cgroup_events=env.get("OO_PARSER_CGROUP_EVENTS", d["cgroup_events"].default),
        )
