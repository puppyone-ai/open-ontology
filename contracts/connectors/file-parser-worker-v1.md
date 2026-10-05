# File parser worker `oo.source.file-parser-worker/v1`

The `oo-file-parser-worker` binary is a child process. It reads one versioned JSON request on stdin, opens one caller-admitted local file, and pulls the public JSONL reader or the public delimited reader. Each record batch leaves as one self-contained Arrow IPC stream, series 60, inside a length-prefixed frame. The matching lineage frame follows that stream. A completion frame is written only after the reader mints its token.

That token is proof of a clean EOF, a matching object SHA-256, and profile coverage. A batch is not that proof. `Exhausted` is not that proof. `admits_import` and `published` stay false. This process does not import, publish, or finish a PostgreSQL run. `pg_run_complete` is false. The receipt is not `PgRunComplete` and not `oo.source.pgsql/v1`.

This is a process, file, and IPC ability. It is not a restricted worker, a loader, or a public endpoint. Kernel network, filesystem, and memory restriction are `unsupported`. The parent owns the kill and the reap. This binary does not install a watchdog, a harness launcher, seccomp, landlock, or a memory rlimit. A blocked `read` is not interrupted by `deadline_ms`. The deadline is the reader's between-read check only.

SP05-W02, AT-FILE-02, and the phase ledger stay open. Arrow IPC here does not admit the new crates to the dependency baseline and does not change the seven-day supply-chain rule.

## Request

Stdin is one JSON document, at most 1,048,576 bytes. The process reads in fixed 8 KiB chunks and stops when another chunk would pass that cap (`request_too_large`). It does not use `read_to_end`. Empty input and invalid UTF-8 are `request_syntax`. Duplicate keys, at any depth, are `duplicate_key`. Trailing bytes after the JSON value are `request_syntax`.

The document is one object. The keys are exactly `protocol`, `input`, `object_sha256`, `profile`, and `budget`. Any other key is `unknown_key`. A missing key, a wrong JSON type, or a wrong enum string is `config`, except the cases below.

`protocol` must be the string `oo.source.file-parser-worker/v1`. Any other string is `unknown_protocol`.

`input` keys are exactly `class`, `admission`, and `path`. `class` must be `local_file`. `admission` must be `caller_admitted_immutable`. That admission is the caller's claim. The process does not prove immutability, does not take a file lease, and does not jail the path.

`path` is absolute, 1 to 4096 bytes, with no NUL, no trailing slash, and no empty, `.`, or `..` component. Anything else is `path_form`. The process does not canonicalize and does not sniff a dialect from the path or the bytes.

`object_sha256` is 64 lowercase hex digits. Uppercase hex is `config`.

These keys are rejected anywhere in the tree, before other unknown-key checks. The token is `credentials`. The key name and the value are not written to stderr: `access_key`, `api_key`, `authorization`, `connection_string`, `credential`, `credentials`, `dsn`, `passwd`, `password`, `private_key`, `secret`, `secret_key`, `token`.

Integers are JSON numbers whose lexical form is optional `-` and digits, with no `+`, no `.`, no exponent, and no leading zero. `deadline_ms` is `1..=3600000`. The deadline starts when the request is accepted (`Instant::now()` plus that duration).

### JSONL profile

`profile.format` is `jsonl`. Keys are exactly `format`, `id`, and `columns`. Dialect, BOM, header, and null keys are `unknown_key` on this profile. Each column's keys are exactly `name`, `type`, and `nullable`. `type` is `utf8`, `bool`, `int64`, or `json_text`. `nullable` is a JSON boolean. The public `JsonlProfile::try_new` rules still apply.

Budget keys are exactly `batch_rows`, `batch_bytes`, `max_record_bytes`, `max_total_bytes`, `max_rows`, `max_columns`, `max_fields`, and `deadline_ms`.

### Delimited profile

`profile.format` is `delimited`. Keys are exactly `format`, `id`, `dialect`, `bom`, `header`, `null`, and `columns`. `dialect` is `csv` or `tsv`. `bom` is `refuse`, `strip`, or `require`. `header` is `absent` or `exact`. There is no sniff mode.

`null` is either `{"policy":"empty"}` or `{"policy":"token","text":"..."}`. `text` is the exact null marker. The key `token` stays in the credential list, so the marker uses `text`. A decimal column's keys are exactly `name`, `type`, `nullable`, `precision`, and `scale`. Every other column's keys are exactly `name`, `type`, and `nullable`. `type` is `utf8`, `bool`, `int64`, or `decimal`. The public `DelimitedProfile::try_new` rules still apply, including precision `1..=38` and scale `-38..=38`.

Budget keys are the JSONL budget keys plus `max_field_bytes`.

## File open

`symlink_metadata` runs first. A directory, symlink, fifo, or other non-regular node is `not_regular` and is not opened. The process then opens the path read-only with `O_NOFOLLOW` (`0x20000` on Linux, `0x0100` on macOS) and checks `metadata` again. A symlink swap between the two checks fails the open. Replacing one regular file with another regular file is not detected. IO failures, including a missing path, are `io`. The OS string and the path are dropped.

## Frames

Stdout is framed binary only. No JSON text and no logs. Each frame is little-endian:

| Bytes | Meaning |
|---|---|
| `u32` | tag |
| `u32` | payload length |
| payload | that many bytes |

| Tag | Name | Payload |
|---|---|---|
| 1 | IPC | one Arrow IPC stream: schema, one record batch, end-of-stream |
| 2 | lineage | UTF-8 lineage preimage for that same batch |
| 3 | complete | UTF-8 completion receipt |

Order for N batches is IPC, lineage, repeated N times, then one complete frame. N may be 0. A complete frame is never written after an error, and an IPC buffer is not written until the whole stream fits the cap. The payload length is the encoded length. An empty payload is not a frame.

### IPC

`arrow-ipc` 60.0.0, default features off. No `lz4` and no `zstd`. `IpcWriteOptions` are alignment 8, legacy IPC false, metadata version V5, compression unset. `StreamWriter::try_new_with_options` writes the schema. `write` writes the one batch. `into_inner` writes the end-of-stream marker.

The cap is `batch_bytes + 65536`. `65536` is the declared fixed overhead for the schema message, alignment padding, and the end-of-stream marker. Before `StreamWriter` is constructed, the process sums `get_array_memory_size` of the batch columns. If that sum is greater than the cap, the token is `ipc_ceiling` and nothing from that batch is written. The public reader has already allocated the `RecordBatch`. This cap binds the IPC sink, not the reader's batch allocator.

`BoundedWriter` starts at `min(1024, cap)` bytes. A `write` that would make the length exceed the cap returns an error before `extend_from_slice`. When more capacity is required, it calls `reserve_exact` for the bytes still needed, and that request is at most the cap. The global allocator may round a reservation up. The length never passes the cap. A stream that still does not fit is `ipc_ceiling`.

Decimal, int64, bool, and UTF-8 values are the Arrow arrays the public readers built. There is no JSON number in the IPC payload, so there is no second rounding step. Null, JSON null, a missing JSONL key, and a delimited empty string stay distinct in lineage. Arrow null is the reader's null, not the text `null`.

### Schema identity

UTF-8, then SHA-256. Field order is the profile order. Metadata keys are sorted per field.

```text
oo-file-parser-schema/v1
field=<name>	<token>	<0|1>
meta=<name>	<key>	<value>
```

Tokens are `utf8`, `bool`, `int64`, and `decimal128:<precision>:<scale>`. `0` is non-null and `1` is nullable. A tab or newline in a name, metadata key, or metadata value is `schema_meta`. A batch whose identity differs from this reconstruction is `schema_drift`. A data type outside the four tokens is `arrow_type`. Zero batches still hash this reconstruction.

The metadata is the metadata the public readers attach: `oo.jsonl.protocol`, `oo.jsonl.logical`, and for `json_text` also `oo.jsonl.encoding=raw-json-utf8`; or `oo.delimited.protocol`, `oo.delimited.logical`, and for decimal also `oo.delimited.precision` and `oo.delimited.scale`.

### Lineage

The same byte cap as the IPC frame. Overflow is `lineage_ceiling`. The preimage does not include cell text.

```text
oo-file-parser-lineage/v1
batch_index=<u64>
reader_protocol=<oo.source.jsonl/v1|oo.source.delimited/v1>
profile_digest=<64 lowercase hex>
config_identity=<64 lowercase hex>
schema_identity=<64 lowercase hex>
raw_bytes=<u64>
arrow_bytes=<u64>
row=<row_index>	<line>	<byte_offset>
field=<name>	<line>	<byte_offset>	<value|null|missing|empty>
```

Digests are the public reader digests. `profile_digest` is SHA-256 of the reader profile preimage. `config_identity` is SHA-256 of that preimage plus `expected_sha256`. Budgets are not in either preimage. JSONL presence is `value`, `null`, or `missing`. Delimited presence is `value`, `null`, or `empty`. A quoted empty delimited field is `empty`, not `null`. An unquoted field that matches the null policy is `null`. A missing JSONL key is `missing`. JSONL field offsets are the opening quote of the key. A missing key uses the record start. Delimited field offsets are the first byte of the field, including a quote when the field is quoted. Line numbers are 1-based. A LF inside quotes increments the line and does not end the record. BOM bytes are in the object hash. After `strip` or `require`, the first record starts at absolute offset 3.

### Completion

The receipt cap is 65536 bytes. It is written only after `finish` returns the reader token, `admits_import` is false, `published` is false, the token's protocol and version constants match the public constants, and `batches` equals the number of IPC frames.

```text
oo-file-parser-complete/v1
protocol=oo.source.file-parser-worker/v1
reader_protocol=<reader protocol>
reader_profile_version=<oo.source.jsonl.profile/v1|oo.source.delimited.profile/v1>
reader_lineage_version=<oo.source.jsonl.lineage/v1|oo.source.delimited.lineage/v1>
reader_receipt_version=<oo.source.jsonl.receipt/v1|oo.source.delimited.receipt/v1>
profile_id=<id>
profile_digest=<64 lowercase hex>
config_identity=<64 lowercase hex>
schema_identity=<64 lowercase hex>
object_sha256=<64 lowercase hex>
rows=<u64>
columns=<u32>
batches=<u64>
ipc_frames=<u64>
lineage_frames=<u64>
admits_import=false
published=false
arrow_ipc=<true|false>
executes_native=false
kernel_network=unsupported
kernel_filesystem=unsupported
kernel_memory=unsupported
parent_kill=caller
parent_reap=caller
launcher=not_this_process
pg_run_complete=false
resumable=false
```

`lineage_frames` equals `ipc_frames`. `arrow_ipc` is `true` only when `ipc_frames` is greater than 0. An empty object that reaches EOF with a matching hash and an absent header, or an empty JSONL stream, completes with zero rows and `arrow_ipc=false`. `header=exact` on an empty file is `header_required` before the hash is compared. A header row whose names match, with no data rows, completes with zero data rows. A hash mismatch can deliver a batch and then fail. That failure does not write a completion frame. `finish` is refused by the reader, and this process exits nonzero.

## Stderr and exit

Stderr is one line, `oo-file-parser-worker: <token>\n`, or nothing on success. The token is a static string. It does not include the path, a key, a cell, or an OS error string. A panic writes `oo-file-parser-worker: internal` and the process still aborts.

| Exit | Tokens |
|---|---|
| 0 | completion frame written |
| 2 | `config`, `request_too_large`, `request_syntax`, `duplicate_key`, `unknown_key`, `unknown_protocol`, `credentials`, `path_form`, `not_regular` |
| 3 | `budget`, `deadline` |
| 4 | `protocol`, `header_required`, `truncated`, `schema_drift`, `arrow_type`, `schema_meta` |
| 5 | `encoding` |
| 6 | `hash_mismatch` |
| 7 | `io` |
| 8 | `ipc_ceiling`, `lineage_ceiling` |
| 9 | `cancelled` |

Reader facts collapse onto these tokens. `header_required` and `truncated` keep their names. Other reader protocol facts, including `malformed_quote`, become `protocol`. Reader budget facts other than `deadline` become `budget`. Reader config facts become `config`.

The process flushes stdout and exits. The caller owns the process group, the timeout, the kill, and the reap. Success means exit 0 and a completion frame. Anything else means a nonzero exit and no completion frame.
