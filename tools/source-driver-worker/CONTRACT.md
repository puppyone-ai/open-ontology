# oo.source.driver-worker/v1

Status: frozen for a later consumer. This delivery is the supervisor, the fixture child, and the parent CLI. It is not Oracle acceptance, not an ODBC or JDBC driver, and not a support-matrix promotion.

The parent process links Arrow IPC, JSON, and SHA-256 only. It does not link a vendor native library or a JVM. The fixture child hashes `driver-artifact.bin` and does not load it.

## Processes

The caller builds one JSON request and one `ParentConfig`. The caller holds the target credential ref. The parent checks pins, then execs the absolute child path from the config or the CLI flag. That path is not a JSON field and is not canonicalized. A symlink is refused. The child environment is empty, the argument list is empty, and stdin is the child JSON only.

On Linux the parent uses fork, then `close_range(3, UINT_MAX, CLOSE_RANGE_CLOEXEC)` in the child after stdio is placed on fds 0, 1, and 2 and before `exec`. That syscall needs Linux 5.11 or newer. The parent process does not close caller-owned descriptors and does not change their flags. Descriptors that survive into the executed image are only the request socket, the frame socket, and the stderr socket. The source credential ref is only in the child JSON on that request socket. If `close_range` returns `ENOSYS`, the parent returns `isolate` and does not exec. There is no path that execs the worker with the inherited table left open.

There is no second worker and no generic-driver fallback.

## Parent CLI

```text
oo-source-driver-parent supervise \
  --worker <absolute-executable> \
  --driver-volume <absolute-directory> \
  --image-digest sha256:<64 lowercase hex> \
  --driver-digest sha256:<64 lowercase hex> \
  --deadline-ms <1..=120000> \
  --request <file mode 0600>
```

`--help` prints this protocol name, `DRIVER_MISSING`, `kernel_network=pending`, and that the program is not Oracle acceptance.

The request file must be a regular file with mode `0600`. Its `image_digest`, `driver.digest`, and `driver.volume` must equal the flags. Exit 0 prints a summary with `rows`, `batches`, `arrow_ipc`, `schema_fingerprint`, and `kernel_network=pending`. That summary has no `secret://` text. Exit 2 prints one stderr line `oo-source-driver-parent: TOKEN`.

## Caller JSON

Exact keys: `protocol`, `source_credential_ref`, `target_credential_ref`, `image_digest`, `driver`, `budget`, `read`.

`protocol` is `oo.source.driver-worker/v1`.

`driver` exact keys: `name`, `volume`, `artifact`, `digest`. `name` is `customer-provided`. `artifact` is `driver-artifact.bin`. Any other artifact name is `DRIVER_MISSING`. Any other driver name or read kind is `read_unsupported`.

`budget` exact keys: `batch_rows`, `batch_bytes`, `max_rows`, `max_bytes`, `deadline_ms`. Integers only. Bounds are `batch_rows` 1..=10000, `batch_bytes` 1..=16777216, `max_rows` 1..=1000000, `max_bytes` 1..=1073741824, `deadline_ms` 1..=3600000. `batch_rows` is at most `max_rows`. `batch_bytes` is at most `max_bytes`.

`read` exact keys: `kind`, `statement_ref`. `kind` is `batch_full`. `statement_ref` is 1..=128 bytes from `[A-Za-z0-9:_-]`.

Credential refs are `secret://` plus 1..=200 bytes from `[A-Za-z0-9/_.-]`. Digests are `sha256:` plus 64 lowercase hex digits. Volume paths are absolute, at most 4096 bytes, with no empty, `.`, or `..` segment and no trailing slash.

Duplicate keys anywhere in the request are `duplicate_key`. These keys are `forbidden_credential` at any depth: `agent_db`, `agent_db_password`, `agent_db_url`, `connection_string`, `gateway`, `gateway_credential`, `gateway_token`, `model`, `model_api_key`, `model_credential`, `passwd`, `password`, `raw_credential`, `secret`, `token`. Values are not scanned.

`target_credential_ref` is kept by the caller. It is not a field of the child JSON. If that string would appear inside the child JSON, the parent returns `forbidden_credential` and does not spawn.

## Child JSON

Exact keys: `protocol`, `source_credential_ref`, `image_digest`, `driver`, `budget`, `read`. The shapes match the caller JSON. `target_credential_ref` is `unknown_key` on this side.

The child reads stdin to EOF, at most 1048576 bytes. `--protocol` prints `oo.source.driver-worker/v1` and exits 0. Any other argument exits 2. Stderr is one line `oo-source-driver-child: TOKEN`.

The fixture child accepts only `fixture:two-rows`, `fixture:wide`, `fixture:slow`, `fixture:crash`, and `fixture:malformed`. Every other statement, including `oracle:select-1`, is `read_unsupported` and does not exec another binary.

## Frames

Little-endian `u32` tag, little-endian `u32` length, then that many payload bytes. Length is checked before the payload is allocated. Length 0 is malformed.

| Tag | Name | Payload |
| --- | --- | --- |
| 1 | Arrow IPC | one self-contained stream: schema, one batch, end of stream |
| 2 | lineage | text below |
| 3 | completion | text below, then clean stdout EOF |

Arrow IPC series 60, alignment 8, legacy IPC false, metadata version V5, compression unset. The v1 schema id is `oo.source.driver-schema/v1` with fields `k:int64:false` and `v:utf8:true`. The fingerprint is the SHA-256 hex of this UTF-8 text, including the final newline:

```text
oo.source.driver-schema/v1
k:int64:false
v:utf8:true
```

Lineage, including the final newline:

```text
batch_index=<u64>
rows=<u64>
arrow_bytes=<usize>
schema_fingerprint=<hex>
```

`arrow_bytes` is the IPC payload length. It must be nonzero and at most `batch_bytes + 65536`. A batch with 0 rows or more than `batch_rows` is `budget`. Cumulative rows and IPC bytes must stay within `max_rows` and `max_bytes`.

Completion, including the final newline:

```text
protocol=oo.source.driver-worker/v1
statement_ref=<statement_ref>
rows=<u64>
batches=<u64>
ipc_frames=<batches>
lineage_frames=<batches>
arrow_ipc=<true if batches > 0 else false>
schema_fingerprint=<hex>
driver_digest=<digest>
image_digest=<digest>
source_credential_forwarded=true
target_credential_forwarded=false
agent_db_credential_forwarded=false
model_credential_forwarded=false
gateway_credential_forwarded=false
fallback=false
admits_import=false
published=false
pg_run_complete=false
kernel_network=pending
vendor_library_loaded=false
```

The parent accepts a receipt only when this text matches, stdout reaches clean EOF, the child exits 0, and `wait` has finished. Exit 0 alone is not a receipt. `finish` before the end frame is `protocol`.

## Pins, deadline, cancel, backpressure

`image_digest` is the SHA-256 of the child executable the parent execs. It is a process pin, not an OCI registry digest. `driver_digest` is the SHA-256 of the regular file `<volume>/driver-artifact.bin`. A missing file or a symlink is `DRIVER_MISSING` before spawn. A digest mismatch is `image_digest` or `driver_digest` before spawn. Files larger than 512 MiB are refused. The parent hashes at most that many bytes.

The parent deadline is 1..=120000 ms and starts before spawn. It covers the digest checks, the request write, the frames, the kill, the reap, and the IO join. The child `budget.deadline_ms` is a separate clock.

The parent reads the next frame only after the caller asks. The pull queue holds one batch. `cancel` and drop set a stop flag, signal the child, and reap. On Linux the group signal is `/bin/kill -KILL -- -<pid>` only when `/proc` start ticks and the process group still identify that child. Otherwise the direct child is killed. `GroupSignal::SkippedIdentity` is not a containment certificate.

## Receipt

`rows`, `batches`, `schema_fingerprint`, and `arrow_ipc` describe the verified frames. `admits_import`, `published`, `pg_run_complete`, `fallback`, and `vendor_library_loaded` are false. `kernel_network` is `pending`.

## Pending

OCI image runtime pinning is not implemented for v1. `kernel_network=pending` is not a pass and not a network policy. This v1 process does not install a network namespace, seccomp filter, or egress policy. A userspace credential check is not that proof. A separate `--network none` harness is evidence about a deployment topology; it is not a claim that this parent enforces routes. Secret resolution (SP02-W04) and the chunk planner (SP03-W03) are not linked. An Oracle child, Oracle SQL, and a schema other than `k`/`v` need a later protocol or schema id. This v1 parent can supervise that child only after those contracts exist. No grade is promoted here.

`oo.source.driver-oracle/v2` is a separate contract in `CONTRACT-V2.md`. It does not change v1 frames, the v1 schema, or `kernel_network=pending`.
