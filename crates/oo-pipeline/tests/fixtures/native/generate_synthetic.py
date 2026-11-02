#!/usr/bin/env python3
"""Generate synthetic decoder fixtures with Python 3.12 + pyarrow==21.0.0.

Committed IPC files are test inputs; normal cargo tests do not need Python/Arrow.
These bytes are not captured source evidence and carry no admission authority.
"""
from datetime import date, datetime, timezone
from decimal import Decimal
from pathlib import Path
import pyarrow as pa

ROOT = Path(__file__).parent


def write(name, fields, arrays):
    batch = pa.RecordBatch.from_arrays(arrays, schema=pa.schema(fields))
    with pa.OSFile(str(ROOT / name), 'wb') as output:
        with pa.ipc.new_stream(output, batch.schema) as writer:
            writer.write_batch(batch)


write('synthetic-standard.arrow', [
    pa.field('id', pa.int64(), nullable=False),
    pa.field('amount', pa.decimal128(38, 9)),
    pa.field('day', pa.date32()),
    pa.field('at', pa.timestamp('s')),
    pa.field('text', pa.string()),
], [
    pa.array([1, 2, 3], pa.int64()),
    pa.array([Decimal('12345678901234567890.123456789'), None, Decimal('-0.000000001')], pa.decimal128(38, 9)),
    pa.array([date(1970, 1, 1), None, date(1969, 12, 31)], pa.date32()),
    pa.array([datetime(1970, 1, 1, 0, 0, 1), None, datetime(1969, 12, 31, 23, 59, 59)], pa.timestamp('s')),
    pa.array(['one', None, ''], pa.string()),
])
union = pa.UnionArray.from_dense(pa.array([0, 1, 0], pa.int8()), pa.array([0, 0, 1], pa.int32()),
                                [pa.array(['lane', '']), pa.array([42], pa.int64())],
                                field_names=['text', 'number'])
arrays = [
    pa.array([b'\0' * 12, b'\1' * 12, b'\xff' * 12], pa.binary(12)),
    pa.array([0, 3, 255], pa.uint8()),
    pa.array([datetime(1970, 1, 1, 0, 0, 1, tzinfo=timezone.utc), None,
              datetime(1969, 12, 31, 23, 59, 59, tzinfo=timezone.utc)], pa.timestamp('ms', 'UTC')),
    pa.array([['a', 'b'], None, []], pa.list_(pa.string())),
    pa.array([{'kind': 'crate', 'n': 7}, None, {'kind': '', 'n': 0}],
             pa.struct([('kind', pa.string()), ('n', pa.int32())])),
    union,
]
write('synthetic-nested.arrow', [pa.field(n, a.type) for n, a in zip(
    ['id', 'presence', 'at', 'marks', 'pack', 'lane'], arrays)], arrays)
