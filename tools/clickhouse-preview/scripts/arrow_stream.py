#!/usr/bin/env python3
"""Decode a ClickHouse ArrowStream of the approved readings columns.

The decoder accepts the five-column stream this source actually writes:
uint64, timestamp nanoseconds UTC, decimal, nullable binary or utf8, int64.
It does not claim to be a general Arrow implementation.
"""

from __future__ import annotations

import json
import struct
from pathlib import Path


class ArrowError(Exception):
    pass


def _u32(buf: bytes, pos: int) -> int:
    return struct.unpack_from("<I", buf, pos)[0]


def _i32(buf: bytes, pos: int) -> int:
    return struct.unpack_from("<i", buf, pos)[0]


def _u16(buf: bytes, pos: int) -> int:
    return struct.unpack_from("<H", buf, pos)[0]


def _i64(buf: bytes, pos: int) -> int:
    return struct.unpack_from("<q", buf, pos)[0]


def _align(pos: int) -> int:
    return pos + (-pos % 8)


class Table:
    def __init__(self, buf: bytes, pos: int) -> None:
        self.buf = buf
        self.pos = pos
        soff = _i32(buf, pos)
        self.vtable = pos - soff

    def offset(self, index: int) -> int | None:
        vsize = _u16(self.buf, self.vtable)
        slot = self.vtable + 4 + index * 2
        if slot + 2 > self.vtable + vsize:
            return None
        field = _u16(self.buf, slot)
        if field == 0:
            return None
        return self.pos + field


def _root(buf: bytes) -> Table:
    if len(buf) < 4:
        raise ArrowError("flatbuffer is short")
    return Table(buf, _u32(buf, 0))


def _vector(buf: bytes, off: int) -> tuple[int, int]:
    absolute = off + _u32(buf, off)
    return absolute, _u32(buf, absolute)


def _string(buf: bytes, off: int) -> str:
    start, size = _vector(buf, off)
    raw = buf[start + 4 : start + 4 + size]
    return raw.decode("utf-8")


def _table_from(buf: bytes, off: int) -> Table:
    return Table(buf, off + _u32(buf, off))


TYPE_NAMES = {
    1: "null",
    2: "int",
    3: "floating",
    4: "binary",
    5: "utf8",
    6: "bool",
    7: "decimal",
    8: "date",
    9: "time",
    10: "timestamp",
    11: "interval",
    12: "list",
    13: "struct",
    14: "union",
    15: "fixedsizebinary",
    16: "fixedsizelist",
    17: "map",
    18: "duration",
    19: "largebinary",
    20: "largeutf8",
}


def _field(buf: bytes, off: int) -> dict:
    table = _table_from(buf, off)
    name_at = table.offset(0)
    type_id_at = table.offset(2)
    type_at = table.offset(3)
    nullable_at = table.offset(1)
    if name_at is None or type_id_at is None:
        raise ArrowError("arrow field is missing a name or type")
    type_id = buf[type_id_at]
    info: dict = {
        "name": _string(buf, name_at),
        "nullable": bool(buf[nullable_at]) if nullable_at is not None else False,
        "type": TYPE_NAMES.get(type_id, f"type{type_id}"),
        "type_id": type_id,
    }
    if type_at is not None and info["type"] == "int":
        body = _table_from(buf, type_at)
        width_at = body.offset(0)
        signed_at = body.offset(1)
        info["bit_width"] = _i32(buf, width_at) if width_at is not None else 0
        info["signed"] = bool(buf[signed_at]) if signed_at is not None else False
    elif type_at is not None and info["type"] == "timestamp":
        body = _table_from(buf, type_at)
        unit_at = body.offset(0)
        zone_at = body.offset(1)
        info["unit"] = struct.unpack_from("<h", buf, unit_at)[0] if unit_at is not None else 0
        info["timezone"] = _string(buf, zone_at) if zone_at is not None else ""
    elif type_at is not None and info["type"] == "decimal":
        body = _table_from(buf, type_at)
        precision_at = body.offset(0)
        scale_at = body.offset(1)
        width_at = body.offset(2)
        info["precision"] = _i32(buf, precision_at) if precision_at is not None else 0
        info["scale"] = _i32(buf, scale_at) if scale_at is not None else 0
        info["bit_width"] = _i32(buf, width_at) if width_at is not None else 128
    return info


def _schema(meta: bytes) -> list[dict]:
    message = _root(meta)
    header_type_at = message.offset(1)
    header_at = message.offset(2)
    if header_type_at is None or header_at is None or meta[header_type_at] != 1:
        raise ArrowError("arrow stream does not start with a schema")
    schema = _table_from(meta, header_at)
    fields_at = schema.offset(1)
    if fields_at is None:
        raise ArrowError("arrow schema has no fields")
    start, count = _vector(meta, fields_at)
    fields = []
    for index in range(count):
        fields.append(_field(meta, start + 4 + index * 4))
    return fields


def _body_length(meta: bytes) -> int:
    message = _root(meta)
    at = message.offset(3)
    if at is None:
        return 0
    return _i64(meta, at)


def messages(data: bytes) -> list[tuple[bytes, bytes]]:
    found: list[tuple[bytes, bytes]] = []
    pos = 0
    while pos + 4 <= len(data):
        mark = _u32(data, pos)
        if mark == 0xFFFFFFFF:
            pos += 4
            if pos + 4 > len(data):
                break
            mark = _u32(data, pos)
        if mark == 0:
            break
        pos += 4
        meta = data[pos : pos + mark]
        if len(meta) != mark:
            raise ArrowError("arrow metadata is truncated")
        pos += mark
        pos = _align(pos)
        body_len = _body_length(meta)
        if body_len < 0 or pos + body_len > len(data):
            raise ArrowError("arrow body is truncated")
        body = data[pos : pos + body_len]
        pos = _align(pos + body_len)
        found.append((meta, body))
    return found


def _header(meta: bytes) -> tuple[int, Table]:
    message = _root(meta)
    type_at = message.offset(1)
    header_at = message.offset(2)
    if type_at is None or header_at is None:
        raise ArrowError("arrow message has no header")
    return meta[type_at], _table_from(meta, header_at)


def _buffers(header: Table) -> list[tuple[int, int]]:
    buf = header.buf
    at = header.offset(2)
    if at is None:
        return []
    start, count = _vector(buf, at)
    values = []
    for index in range(count):
        base = start + 4 + index * 16
        values.append((_i64(buf, base), _i64(buf, base + 8)))
    return values


def _batch_length(header: Table) -> int:
    at = header.offset(0)
    if at is None:
        raise ArrowError("record batch has no length")
    return _i64(header.buf, at)


def _validity(body: bytes, offset: int, length: int, rows: int) -> list[bool]:
    if length == 0:
        return [True] * rows
    flags = []
    for index in range(rows):
        byte = body[offset + index // 8]
        flags.append(bool(byte & (1 << (index % 8))))
    return flags


def _buffer_count(field: dict) -> int:
    kind = field["type"]
    # ClickHouse writes a validity buffer for every column. Length 0 means no nulls.
    if kind in {"int", "timestamp", "decimal", "floating", "bool", "date", "time"}:
        return 2
    if kind in {"binary", "utf8", "largebinary", "largeutf8"}:
        return 3
    raise ArrowError(f"arrow type {kind} is not a readings column")


def _decode_column(field: dict, body: bytes, buffers: list[tuple[int, int]], rows: int) -> list[object]:
    needed = _buffer_count(field)
    if len(buffers) < needed:
        raise ArrowError(f"column {field['name']} is missing buffers")
    used = buffers[:needed]
    del buffers[:needed]
    offset, length = used[0]
    valid = [True] * rows if length == 0 else _validity(body, offset, length, rows)
    cursor = 1
    kind = field["type"]
    if kind == "int":
        offset, length = used[cursor]
        width = int(field.get("bit_width") or 64) // 8
        signed = bool(field.get("signed"))
        values: list[object] = []
        for index in range(rows):
            if not valid[index]:
                values.append(None)
                continue
            raw = body[offset + index * width : offset + (index + 1) * width]
            if len(raw) != width:
                raise ArrowError(f"column {field['name']} value is short")
            values.append(int.from_bytes(raw, "little", signed=signed))
        return values
    if kind == "timestamp":
        if int(field.get("unit") or 0) != 3:
            raise ArrowError("timestamp unit is not nanoseconds")
        offset, _length = used[cursor]
        values = []
        for index in range(rows):
            if not valid[index]:
                values.append(None)
                continue
            values.append(_i64(body, offset + index * 8))
        return values
    if kind == "decimal":
        width = int(field.get("bit_width") or 128) // 8
        scale = int(field.get("scale") or 0)
        offset, _length = used[cursor]
        values = []
        for index in range(rows):
            if not valid[index]:
                values.append(None)
                continue
            raw = body[offset + index * width : offset + (index + 1) * width]
            if len(raw) != width:
                raise ArrowError(f"decimal {field['name']} is short")
            number = int.from_bytes(raw, "little", signed=True)
            sign = "-" if number < 0 else ""
            digits = f"{abs(number):0{scale + 1}d}"
            whole, frac = digits[:-scale], digits[-scale:]
            values.append(f"{sign}{whole}.{frac}" if scale else f"{sign}{whole}")
        return values
    if kind in {"binary", "utf8", "largebinary", "largeutf8"}:
        wide = kind.startswith("large")
        off_size = 8 if wide else 4
        offsets_at, _olen = used[cursor]
        data_at, _dlen = used[cursor + 1]
        values = []
        for index in range(rows):
            if not valid[index]:
                values.append(None)
                continue
            if wide:
                begin = _i64(body, offsets_at + index * 8)
                end = _i64(body, offsets_at + (index + 1) * 8)
            else:
                begin = _i32(body, offsets_at + index * off_size)
                end = _i32(body, offsets_at + (index + 1) * off_size)
            if begin < 0 or end < begin:
                raise ArrowError(f"column {field['name']} offset is invalid")
            values.append(body[data_at + begin : data_at + end])
        return values
    raise ArrowError(f"arrow type {kind} is not a readings column")


def rows_from_stream(data: bytes) -> list[dict]:
    parts = messages(data)
    if not parts:
        raise ArrowError("arrow stream is empty")
    fields = _schema(parts[0][0])
    records: list[dict] = []
    for meta, body in parts[1:]:
        kind, header = _header(meta)
        if kind == 2:
            raise ArrowError("dictionary batches are not a readings column encoding")
        if kind != 3:
            continue
        count = _batch_length(header)
        buffers = _buffers(header)
        columns = []
        for field in fields:
            columns.append(_decode_column(field, body, buffers, count))
        if buffers:
            raise ArrowError("arrow record batch has unused buffers")
        for index in range(count):
            records.append({field["name"]: columns[pos][index] for pos, field in enumerate(fields)})
    return records


def dump(path: Path) -> list[dict]:
    decoded = rows_from_stream(path.read_bytes())
    printable = []
    for row in decoded:
        item = {}
        for key, value in row.items():
            if isinstance(value, bytes):
                item[key] = {"hex": value.hex()}
            else:
                item[key] = value
        printable.append(item)
    return printable


if __name__ == "__main__":
    import sys

    print(json.dumps(dump(Path(sys.argv[1])), indent=2))
