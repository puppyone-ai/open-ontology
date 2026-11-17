"""Per-chunk result cache (D73): key = BLAKE3(PCM) || interval || model revision || parameters.

The cache lives in the job directory under the scratch and survives a killed worker, so a retry only
computes the chunks that have no entry. Entries are written with tmp-and-rename: a kill never leaves a half file.
"""
import hashlib
import json
import os
from pathlib import Path

try:
    from blake3 import blake3 as _blake3
except ImportError:  # dev hosts only; the image has blake3 (requirements.in)
    _blake3 = None


def digest(data: bytes) -> str:
    if _blake3 is not None:
        return _blake3(data).hexdigest()
    return "sha256-" + hashlib.sha256(data).hexdigest()


def chunk_key(pcm: bytes, start_ms: int, end_ms: int, model_revision: str, params: dict) -> str:
    p = json.dumps(params, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return digest(pcm + b"\0" + f"{start_ms}-{end_ms}".encode() + b"\0" + model_revision.encode() + b"\0" + p.encode())


class ChunkCache:
    def __init__(self, root):
        self.root = Path(root)
        self.root.mkdir(parents=True, exist_ok=True)

    def get(self, key):
        p = self.root / f"{key}.json"
        try:
            return json.loads(p.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return None

    def put(self, key, value):
        tmp = self.root / f".{key}.tmp"
        tmp.write_text(json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")), encoding="utf-8")
        os.replace(tmp, self.root / f"{key}.json")

    def keys(self):
        return sorted(p.stem for p in self.root.glob("*.json"))
