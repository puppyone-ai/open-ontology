# File worker parent `oo.source.file-worker-parent/v1`

The parent supervises one trusted `oo-file-parser-worker` child. The caller supplies an absolute compiled worker path and one bounded request. The parent spawns that path directly, writes the worker JSON, and pulls framed Arrow IPC. `ParentParseComplete` is returned only after every batch the caller pulled was verified, the completion frame matches the observed stream, stdout reaches a clean EOF, the child exits 0, and `wait` has observed that exit.

`admits_import`, `published`, and `pg_run_complete` are false. The parent does not mint `JsonlComplete`, `DelimitedComplete`, or a PostgreSQL `RunComplete`. A batch is not the receipt. `Exhausted` is not the receipt. Exit 0 alone is not the receipt.

This is not a restricted worker and not a public endpoint. `KERNEL_RESTRICTION` is `Unsupported`. That value is not a containment certificate. The parent does not install seccomp, landlock, a memory rlimit, or `runsc`. A descendant that calls `setsid` leaves the child's process group and is not killed. Inherited extra file descriptors are not closed. Those limits are the same ones the parser supervisor records. This parent does not add an attack campaign to re-prove them.

SP05-W02, AT-FILE-02, and the phase ledger stay open.

## Deadline

`ParentConfig` takes `deadline_ms` in `1..=120000`. `0` and `120001` are `deadline` and do not spawn. The clock starts before `spawn`. The same budget covers the file hash, the request write, frame reads, the kill, the reap, and the IO-thread join.

Socket waits use `UnixStream` timeouts of at most 20 ms, and shorter when less than 20 ms remains. The IO thread checks the deadline on each slice. After a kill, `join` returns because a blocked read or send wakes on that slice. The reap loop after the signal is at most 1000 ms. It does not call a blocking `wait`.

The worker JSON `deadline_ms` is a different clock. It is the child's between-read check, `1..=3600000`. The parent deadline is the outer bound. The parent does not extend either clock.

## Worker path

`ParentConfig::try_new` stores the path. The request has no executable field. The path must be absolute, 1 to 4096 bytes, with no NUL, no trailing slash, and no empty, `.`, or `..` component. `symlink_metadata` must show a regular file with an execute bit. A symlink is refused. The path is not canonicalized and is not passed to a shell. `Command` uses that path, an empty environment, no arguments, and a private empty working directory.

## Request

The input is one caller-admitted local file. `ChildRequest::try_new` requires the same absolute path form and a regular file via `symlink_metadata`. The admission string written to the child is `caller_admitted_immutable`. The parent does not prove the file stays unchanged. A symlink, directory, fifo, or other node is `not_regular` and does not spawn.

The parent hashes the file with SHA-256 before spawn, up to `max_total_bytes`. A larger file is `input_too_large` and does not spawn. The parent still spawns when the request hash differs from the file hash, and that call cannot return `ParentParseComplete`.

The JSON keys are the worker contract: `protocol`, `input`, `object_sha256`, `profile`, and `budget`. `input.class` is `local_file`. Profile and budget come from the public JSONL or delimited constructors. A JSONL profile with a delimited budget is `profile_budget`. The encoded request is at most 1,048,576 bytes. No credential key is written.

## Frames

Stdout is the worker frame layout. Tag 1 is one Arrow IPC stream, tag 2 is the lineage for that stream, tag 3 is the completion frame. The parent reads a pair only after the caller asks for the next batch. It decodes the IPC payload with `arrow_ipc::reader::StreamReader` and accepts one record batch followed by end-of-stream. A second batch, a short frame, an unknown tag, or a length of 0 is `malformed`. The length is checked before the payload is allocated. IPC and lineage payloads may not exceed `batch_bytes + 65536`. The completion payload may not exceed 65536 bytes.

The parent rebuilds the schema from the public profile, including the worker metadata keys, and hashes `oo-file-parser-schema/v1`. The decoded schema identity must match. Lineage must be the worker preimage: batch index, reader protocol, profile digest, config identity, schema identity, raw bytes, arrow bytes, then one row group per record. Profile digest and config identity are the public reader functions. The lineage `arrow_bytes` value is the worker's pre-encode sum of `get_array_memory_size`. IPC decode changes buffer lengths, so that sum is not required to equal the decoded arrays. Both the lineage value and the decoded sum must be at most `batch_bytes + 65536`, and both must be non-zero when the batch has rows. Field names and presence tokens must match the profile. JSONL presence is `value`, `null`, or `missing`. Delimited presence is `value`, `null`, or `empty`.

The completion text must match the worker receipt line for line, with the observed row count and batch count. `ipc_frames` and `lineage_frames` equal that count. `arrow_ipc` is `true` only when the count is greater than 0. `admits_import=false`, `published=false`, and `pg_run_complete=false` are required text, not caller input. After the completion frame, the next stdout read must be EOF. Any further byte is `trailer` and is not a receipt. A completion hash that differs from the request hash or the file hash is `hash_mismatch`.

## Receipt

`ParentPull::next_batch` returns one `VerifiedBatch` or `Ok(None)` after the verified completion and the clean EOF. `Ok(None)` is not the receipt. `ParentPull::finish` waits for the child. It returns `ParentParseComplete` only when that wait reports exit 0 and no signal. `finish` before `Ok(None)` returns `protocol` and does not build the receipt. Drop kills and reaps. The receipt methods `admits_import`, `published`, and `pg_run_complete` return false.

Timeout, nonzero exit, a malformed frame, a trailer, a hash mismatch, a protocol mismatch, and an early completion frame all return `ParentError`. None of those paths returns `ParentParseComplete`.

## Kill and reap

The child is started with process group 0, so its pid is the group leader. On timeout or drop, the parent reads `/proc/<pid>/stat` on Linux. It signals the group only when the pid is greater than 1, the start-time ticks still match the ticks read just after spawn, the process-group id is that pid, and the group is not the parent's group. The command is `/bin/kill`, with arguments `-KILL`, `--`, and `-<pid>`. It is not a shell and it is not `kill -s KILL`. If the identity check fails, the group is not signaled. `Child::kill` still signals the direct child when `try_wait` reports that this parent still owns it. A pid is not reused before that wait. A `setsid` descendant is outside the group and survives. That is `SkippedIdentity` or a live grandchild, not a successful containment proof.

## Errors

| Kind | When |
|---|---|
| `spawn` | The process or a socket could not be created. |
| `timed_out` | The parent deadline expired. |
| `malformed` | Tag, length, or Arrow stream is not the worker frame. |
| `trailer` | A byte follows the completion frame. |
| `hash_mismatch` | Request, file, and completion hashes disagree, or the child token is `hash_mismatch`. |
| `protocol` | Role, profile, order, schema, count, lineage, or completion binding failed, or `finish` ran early. |
| `ended` | Stdout closed before a completion frame. |
| `worker` | Nonzero exit, or exit 0 without a verified end. |
| `io` | A pipe or the IO thread failed. |

Stderr is kept only to recognize the worker's static token list. The token bytes, the path, and the cell text are not part of `Display`.
