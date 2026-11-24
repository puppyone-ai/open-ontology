"""Content-addressed scratch directory with an LRU size cap (10 §2.6).

Blobs are files named by their sha256. Uploads are written to `*.part`, hashed
while streaming and renamed only when the hash matches. A blob in use by a job
is pinned and never evicted.
"""
import hashlib
import os
import re
import threading
from collections import OrderedDict
from contextlib import contextmanager
from pathlib import Path

HEX64 = re.compile(r"^[0-9a-f]{64}$")


class ScratchFull(Exception):
    pass


class Scratch:
    def __init__(self, root, cap_bytes):
        self.root = Path(root)
        self.cap = cap_bytes
        self.root.mkdir(parents=True, exist_ok=True)
        self._lock = threading.Lock()
        self._lru = OrderedDict()  # sha -> size, least recently used first
        self._pins = {}  # sha -> count
        self._reserved = 0  # bytes of uploads in flight
        found = []
        for p in self.root.iterdir():
            if p.suffix == ".part":
                p.unlink(missing_ok=True)
            elif HEX64.match(p.name) and p.is_file():
                st = p.stat()
                found.append((st.st_mtime, p.name, st.st_size))
        for _, name, size in sorted(found):
            self._lru[name] = size

    def path(self, sha):
        return self.root / sha

    def total(self):
        with self._lock:
            return sum(self._lru.values()) + self._reserved

    def count(self):
        with self._lock:
            return len(self._lru)

    def has(self, sha):
        with self._lock:
            return sha in self._lru and self.path(sha).exists()

    @contextmanager
    def use(self, sha):
        """Pin `sha` for the duration of a job; yields the path or None."""
        with self._lock:
            ok = sha in self._lru and self.path(sha).exists()
            if ok:
                self._pins[sha] = self._pins.get(sha, 0) + 1
                self._lru.move_to_end(sha)
                os.utime(self.path(sha))
        try:
            yield self.path(sha) if ok else None
        finally:
            if ok:
                with self._lock:
                    self._pins[sha] -= 1
                    if not self._pins[sha]:
                        del self._pins[sha]

    def delete(self, sha):
        with self._lock:
            if self._pins.get(sha):
                return False
            self._lru.pop(sha, None)
        self.path(sha).unlink(missing_ok=True)
        return True

    def _reserve(self, size):
        with self._lock:
            used = sum(self._lru.values()) + self._reserved
            for sha in list(self._lru):
                if used + size <= self.cap:
                    break
                if self._pins.get(sha):
                    continue
                used -= self._lru.pop(sha)
                self.path(sha).unlink(missing_ok=True)
            if used + size > self.cap:
                raise ScratchFull()
            self._reserved += size

    def put(self, sha, size, read):
        """Store a blob of `size` bytes pulled with `read(n) -> bytes`.

        Returns ("created" | "exists", None) or ("mismatch" | "short", None).
        Raises ScratchFull when the cap cannot be met even after eviction.
        """
        self._reserve(size)
        part = self.root / f"{sha}.{threading.get_ident()}.part"
        h = hashlib.sha256()
        got = 0
        try:
            with open(part, "wb") as f:
                while got < size:
                    chunk = read(min(1 << 20, size - got))
                    if not chunk:
                        return "short", None
                    h.update(chunk)
                    f.write(chunk)
                    got += len(chunk)
            if h.hexdigest() != sha:
                return "mismatch", None
            with self._lock:
                existed = sha in self._lru and self.path(sha).exists()
                if existed:
                    self._lru.move_to_end(sha)
                else:
                    os.replace(part, self.path(sha))
                    self._lru[sha] = size
            return ("exists" if existed else "created"), None
        finally:
            part.unlink(missing_ok=True)
            with self._lock:
                self._reserved -= size
