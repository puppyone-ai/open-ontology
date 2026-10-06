# Findings

Reviewed the frozen file-worker parent at `bf30b717029d0715e0a1abb7fc8ec02ab7630534`. The decisive process is `linux/overlay-4`. The author smoke before that harness existed is `linux/preoverlay`.

## Production pull

- CSV `a237-lot`, TSV `a237-tab`, and JSONL `a237-notes` complete through `oo-file-parser-worker` with clean EOF. Empty JSONL completes with zero batches, `arrow_ipc=false`, and columns 3. The recovery file after the deadline kill completes with one row.
- CSV values are `甲` / empty / `鹿`, `i64::MIN` / `i64::MAX` / `0`, and decimal coefficients `3140` / `3140` / `-10` at scale 3. Quoted empty is `empty`. The token `~` is `null`. The quoted field `x\ny` is `value` and its lineage line is 4.
- TSV keeps a quoted tab and a comma inside a field. Header policy is absent, so `raw_bytes` equals the file length. Unquoted `-0` is stored as `0`.
- JSONL uses `batch_rows` 1 and three IPC frames. Label presence is `value`, `null`, then `missing`. The null offset is the opening quote of the key. The missing offset is `{`. Per-batch `raw_bytes` excludes the trailing newline.
- Receipt profile and config digests match the public `profile_digest` and `config_identity` methods. `reader_protocol` is `oo.source.delimited/v1` or `oo.source.jsonl/v1`. `admits_import`, `published`, and `pg_run_complete` are false. `KERNEL_RESTRICTION` is `Unsupported`.
- On each non-empty batch, lineage `arrow_bytes` and the decoded array-memory sum are non-zero and at most `1_048_576 + 65_536`. Equality of those two sums is not required.

## Refusal and frames that are not receipts

- Deadline `0` and `120001` are `Config("deadline")` and do not spawn. A relative worker, a symlink worker, a dotted absolute input, a symlink input, and a profile/budget mismatch are config errors and do not spawn.
- A wrong object hash spawns the worker and returns `HashMismatch`, token `hash_mismatch`, exit 6, reaped. Display text hides the path and the planted cell `a237-secret-cell`.
- An unterminated quote exits 4, reaped, and is not `TimedOut`. Display text hides the path and `a237-open`.
- No stdout and exit 0 is `Ended`, exit 0, reaped.
- A completion-length frame with a non-receipt payload and one trailing byte is `Trailer`. `read_clean_eof` runs before the completion payload is bound, so the trailing byte wins. Display text hides the payload.

## Deadline and the next call

- An 800 ms deadline around a sleeping fixture returns `TimedOut`, `term_signal` `Some(9)`, `GroupSignal::Sent`, reaped, and `pid_alive` false. Overlay-4 wall time is 835 ms.
- The following call uses the production worker on a new JSONL file and returns `ParentParseComplete`. The killed pull does not block that call.

## Group signal

- `GroupSignal::Sent` means `/bin/kill -KILL -- -<pid>` returned success after the `/proc` identity check. The direct child is reaped.
- A `setsid` grandchild was session leader pid 189. It was still in `/proc` after the parent returned `TimedOut`. This review killed that pid and saw `/proc/189` go away. The parent does not contain a new session. `KERNEL_RESTRICTION` stays `Unsupported`. Inherited extra file descriptors are not closed by this parent. The reap after the signal is bounded `try_wait`.

## Packages

`Cargo.lock` and `crates/oo-connectors/Cargo.toml` are byte-identical to parent `543580768dec60d127eb428c09e0a9a400b18272`. This review did not edit dependencies and did not rerun the arrow supply audit. a207 remains the sole writer of that manifest and the root lock.

## Preserved harness failures

- `linux/overlay` `test.exit` 101: temporary `Schema` dropped while borrowed. Container `36320a81d02d`.
- `linux/overlay-2` `test.exit` 101: `let schema` shadowed the schema-identity `Option`. Container `da25d89cdac3`.
- `linux/overlay-3` tests and the hash compare exited 0. Clippy `while_let_loop` and `let_and_return` exited 101. Container `1d86f3510db3`.

The product sources were the same archive in every run.

## Left in place

`sqlx-postgres` `message/flush.rs` and `message/mod.rs` emit `unfulfilled_lint_expectations` while that dependency is compiled. Those files were not edited. Preoverlay and overlay-4 clippy still exited 0.

## Open

SP05-W02, AT-FILE-02, and the phase ledger stay unchecked. This parent is not an importer, a publisher, a loader, or a public endpoint. PostgreSQL was not part of this run. The limited approval is the parent supervision observed above.
