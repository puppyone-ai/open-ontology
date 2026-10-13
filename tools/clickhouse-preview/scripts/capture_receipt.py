#!/usr/bin/env python3
"""Build an oo.source-admission.capture-receipt/v1 file from a real capture.

The receipt bytes and the native load document carry the same engine, source,
generation, snapshot, and digests. declared_receipt_sha256 is the sha256 of
the file this module writes. Digests are hashes of files and Arrow bytes, not
typed constants. The file must not contain a secret_ref key.
"""

from __future__ import annotations

import hashlib
import json
import struct
from pathlib import Path

PROTOCOL = "oo.source-admission.capture-receipt/v1"
ARROW_MAGIC = b"ARROW1"


class ReceiptError(Exception):
    pass


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def _i32(buf: bytes, pos: int) -> int:
    return struct.unpack_from("<i", buf, pos)[0]


def _u16(buf: bytes, pos: int) -> int:
    return struct.unpack_from("<H", buf, pos)[0]


def _u32(buf: bytes, pos: int) -> int:
    return struct.unpack_from("<I", buf, pos)[0]


def _i64(buf: bytes, pos: int) -> int:
    return struct.unpack_from("<q", buf, pos)[0]


def _align8(pos: int) -> int:
    return pos + (-pos % 8)


class _Table:
    def __init__(self, buf: bytes, start: int) -> None:
        soff = _i32(buf, start)
        vtable = start - soff
        vsize = _u16(buf, vtable)
        if vsize < 4 or vsize % 2 != 0:
            raise ReceiptError("arrow vtable is closed")
        self.buf = buf
        self.start = start
        self.vtable = vtable
        self.vsize = vsize

    def field(self, index: int) -> int | None:
        slot = 4 + index * 2
        if slot + 2 > self.vsize:
            return None
        rel = _u16(self.buf, self.vtable + slot)
        if rel == 0:
            return None
        return self.start + rel


def _root(meta: bytes) -> _Table:
    if len(meta) < 4:
        raise ReceiptError("arrow metadata is short")
    return _Table(meta, _u32(meta, 0))


def _body_length(meta: bytes) -> int:
    table = _root(meta)
    at = table.field(3)
    if at is None:
        return 0
    return _i64(meta, at)


def _schema_meta(meta: bytes) -> bool:
    if not meta:
        return False
    table = _root(meta)
    at = table.field(1)
    return at is not None and meta[at] == 1


def _frames(data: bytes, require_eos: bool) -> list[tuple[bytes, bool]]:
    offset = 0
    found: list[tuple[bytes, bool]] = []
    while offset + 8 <= len(data):
        continuation = _i32(data, offset)
        meta_len = _i32(data, offset + 4)
        if continuation != -1 or meta_len < 0:
            raise ReceiptError("arrow frame is truncated")
        if meta_len == 0:
            if offset + 8 != len(data):
                raise ReceiptError("arrow eos is not terminal")
            found.append((b"", True))
            return found
        meta_end = offset + 8 + meta_len
        if meta_end > len(data):
            raise ReceiptError("arrow metadata is truncated")
        meta = data[offset + 8 : meta_end]
        body_len = _body_length(meta)
        if body_len < 0:
            raise ReceiptError("arrow body length is closed")
        body_start = _align8(meta_end)
        body_end = body_start + body_len
        if body_end > len(data):
            raise ReceiptError("arrow body is truncated")
        found.append((meta, False))
        nxt = body_end
        if nxt % 8 != 0:
            aligned = _align8(nxt)
            if aligned > len(data) or any(data[pos] != 0 for pos in range(nxt, aligned)):
                raise ReceiptError("arrow padding is truncated")
            nxt = aligned
        offset = nxt
    if offset != len(data) or require_eos:
        raise ReceiptError("arrow stream did not end on a frame")
    return found


def _file_interior(data: bytes) -> bytes:
    if len(data) < 16 or data[-6:] != ARROW_MAGIC:
        raise ReceiptError("arrow file magic is absent")
    size_at = len(data) - 10
    footer_len = _i32(data, size_at)
    if footer_len <= 0:
        raise ReceiptError("arrow footer is closed")
    footer_start = size_at - footer_len
    if footer_start < 8:
        raise ReceiptError("arrow footer overlaps the magic")
    at = 6
    if at % 8 != 0:
        aligned = _align8(at)
        if aligned > footer_start or any(data[pos] != 0 for pos in range(at, aligned)):
            raise ReceiptError("arrow file padding is truncated")
        at = aligned
    while at + 8 <= footer_start:
        if _i32(data, at) == -1:
            break
        if any(data[pos] != 0 for pos in range(at, at + 8)):
            raise ReceiptError("arrow file padding is truncated")
        at += 8
    if at + 8 > footer_start:
        raise ReceiptError("arrow file has no frames")
    return data[at:footer_start]


def arrow_schema_sha256(data: bytes) -> str:
    """sha256 of the schema message metadata. Not the stream digest."""
    if data.startswith(ARROW_MAGIC):
        frames = _frames(_file_interior(data), False)
    else:
        frames = _frames(data, True)
    for meta, eos in frames:
        if eos or not meta:
            continue
        if _schema_meta(meta):
            return sha256_bytes(meta)
    raise ReceiptError("arrow schema message is absent")


def canonical_bytes(document: dict) -> bytes:
    return json.dumps(document, sort_keys=True, separators=(",", ":")).encode("utf-8") + b"\n"


def refuse_secret(document: dict) -> None:
    raw = json.dumps(document)
    if "secret_ref" in raw or "secret://" in raw:
        raise ReceiptError("receipt would contain a secret reference")


def write_receipt(path: Path, body: dict) -> str:
    refuse_secret(body)
    if body.get("schema") != PROTOCOL:
        raise ReceiptError("receipt schema is not the capture protocol")
    payload = canonical_bytes(body)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(payload)
    return sha256_bytes(payload)
