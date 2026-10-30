# C5 growing-range transport (implementation contract)

Status: owned MySQL 5.7.28 source/worker/target/native-publication and two real
SIGKILL recovery paths passed. Not installed in the customer runtime; this is
not a Gateway or business-answer acceptance receipt.

Business row counts have no product-defined ceiling. Existing `max_rows` and
`max_bytes` bound each source page/Arrow part, not the entire logical range.
Never increase a historical sealed admission budget to disguise growth.

The source captures a growing logical range in one read-only consistent
snapshot, using the existing source-collation keyset ordering. It writes bounded
Arrow parts and an append-only part index. Nothing is published until every part
has been independently admitted, staged and checked. Target recovery reuses
durable parts. Loss of the source snapshot restarts only the affected range,
preserving the failed capture; it is not a whole-database fallback.

## Frozen source/target boundary

New source output uses `protocol: oo.mysql.c5-range-parts/v1`.
Keep the existing range identity, bounds, source/parent generations, keys,
type/normalization policy and observation fields. Do not return the old
`oo.pipeline.native-c5-range/v1` envelope for a multipart capture.

The output replaces the single `arrow_path`, `arrow_sha256`, `members` and
`members_file` with:

```json
{
  "parts_index": {
    "path": "/private/capture/parts.jsonl",
    "sha256": "<sha256 of exact index bytes>",
    "bytes": 1234
  },
  "part_count": 2,
  "row_count": 100001,
  "input_digest": "<same index sha256>"
}
```

Each index line is a complete JSON object, followed by LF:

```json
{
  "ordinal": 0,
  "arrow": {
    "path": "/private/capture/part-0.arrow",
    "sha256": "<sha256 of exact part bytes>",
    "bytes": 1234
  },
  "row_count": 100000
}
```

Ordinals are contiguous from zero; `part-N.arrow` must match the ordinal and
reside beside the index. A zero-row replacement has one schema-carrying empty
Arrow part. There is no fixed maximum part count or cumulative row count.
Consumers stream index lines; they must not load all Arrow parts into memory.

Keep source capture witnesses and audit evidence for each part. All parts share
one real snapshot, source identity, schema/type projection and generation.
The outer index is not an admission receipt. The real source-admission sealer
still authenticates source capture evidence. Target staging extracts typed keys
from admitted Arrow, checks duplicates across parts and reconciles the complete
parent-range membership atomically. No tombstones, public generation or ACK may
advance from a partial index.

Original single-part protocol remains accepted for legacy captures. New tests
must exceed one page, include exact-page boundaries, empty final replacements,
text/binary/composite keys, growth/update/delete, corrupt/missing/reordered parts,
interrupted capture, target staging restart and final Gateway correctness.

## Admitted transport and target application

`layered-producer.py` seals each part separately, then writes `admitted_index`
as private, hash-bound JSONL. Each line has exactly `scope`, `registry`,
`native_request`, `type_policy` and `normalization_rejects`. A compact operator
descriptor pins that index; it is not an aggregate admission seal.

The loader accepts `oo.pipeline.native-c5-range-parts/v1`, authenticates every
part, and stages bounded typed rows using migration 0103. It verifies complete
index coverage before migration 0104 performs one atomic range replacement,
exact staged-value reconciliation and membership/tombstone updates. No array
of all range members and no copy of the entire unchanged parent is required.
Source column names use the existing canonical target mapping. Masked
normalization evidence is bound to the admitted capturer and retained through
the existing quality API.

The SQL scope replaces legacy `members` with the actual `parts_task`; its
`arrow_sha256` is the complete source index digest. A resumed task reuses
durable admitted parts. After a committed target transaction, the rebind API
requires the same actual task and stored receipts before moving its lease
generation; it returns the real deleted-row count rather than fabricating an
empty replay result.

0103/0104 are new installations, never edits to installed 0075/0100/0101/0102.
0104 supports C5-only targets without 0100; if MySQL identity metadata is
present, its complete job/schema authority is mandatory. Partial identity
installations fail closed.
