# Findings

Reviewed the frozen parse-file scope against `contracts/connectors/parse-file-cli-v1.md`, the public parent API, and the build07 proof. This is not an acceptance review and it does not close a separate review slot.

## Observed

- Pre-overlay smoke on `1ddbd865fc77c7a1a6209949336f592107d77683` passed before the three overlay paths existed in the extract. `file_parser_worker` was 12 passed in 0.96s. `file_worker_parent` was 8 passed in 1.01s. The parent stall line was `stall_elapsed_ms=832 kind=TimedOut exit=None signal=Some(9) group=Sent reaped=true alive=false`.
- After the overlay, `parse_file_cli` passed 12 tests in 221.04s on the compiled `oo-file-parser-worker`: help and unknown arguments; request shape, credentials, worker path, worker deadline, unknown budget field, and dialect disagreement; CSV, TSV, and JSONL summaries; empty JSONL; a malformed quote with worker token `protocol` and child exit 4; input hash mismatch with child exit 6; exact header on an empty file with worker token `header_required` and child exit 4; executable SHA mismatch with `reaped` false; parent deadline then a later empty JSONL call.
- The CSV run checks public `DelimitedProfile` digests, a stable 64-hex `schema_identity`, and the absence of cell text from stdout and stderr. The tests do not read Arrow arrays.
- Clippy `cargo clippy --locked --offline -p oo-connectors --bin parse-file --test parse_file_cli -- -D warnings` exited 0 on Rust 1.94.0. Checking the package printed two pre-existing `sqlx-postgres` unfulfilled `dead_code` expectations. Those warnings are outside this diff.
- Source blob SHA-256 matches `linux/build07/overlay-sha256.txt`. No manifest, lock, parent, or worker edit.

## Deadline on the production worker

The parent stall fixture uses a sleeper and is not this command's fixture. These runs used `oo-file-parser-worker`.

- build02 asked for signal 9 from a 1 ms parent deadline. The assertion left `term_signal` null and right 9. A 1 ms clock expires during spawn, before the request is written. The worker then sees stdin EOF. That attempt is preserved and was not repeated.
- build04 used 2000000 rows and `batch_rows` 65536. The first lineage frame exceeded the worker frame cap. The child exited 8 with token `lineage_ceiling`, `group_signal` `not_needed`, and the command exited 5 with fact `ended`. The fixture moved to 40000 rows and `batch_rows` 16 so one field's lineage frame fits.
- build05 calibrated an empty JSONL at 9073 ms and a 40000-row parse at 61623 ms, then set the kill deadline to the whole gap. The kill wall was 61719 ms, so the deadline did not cut the parse.
- build06 used half that gap. `empty_ms=8975`, `busy_ms=62859`, `kill_ms=26941`, `kill_elapsed_ms=35861`. The error document was `{"exit_code":7,"fact":"timed_out","group_signal":"sent","outcome":"deadline","protocol":"oo.connectors.parse-file-error/v1","reaped":true,"term_signal":null,"worker_token":null}`. The parent had sent the group signal. The worker still exited 7, which matches a closed-pipe `Fail::io` winning the race with `wait` for SIGKILL. The strict `term_signal == 9` assertion failed.
- build07 accepts either termination signal 9 with a null child exit, or a null termination signal with child exit 7. It still requires outcome `deadline`, fact `timed_out`, `reaped` true, `group_signal` `sent`, a null worker token, a shorter wall than the full parse, and a following empty JSONL success. That test passed. The passing run does not print which of the two teardowns occurred. Signal 9 is not a guaranteed observation for this worker.

## Limits

- `KernelRestriction::Unsupported` is the value this command prints. `group_signal` `sent` is not a containment certificate. This task added no `setsid` campaign, seccomp, landlock, or rlimit.
- The proof is linux/arm64 Rust 1.94.0. It did not run on another OS.
- The executable hash and `ParentConfig::start` are not one snapshot. The command says so in the contract.
- `schema_identity` is taken from `ParentParseComplete`. The test checks that it is stable 64-hex and that `profile_digest` and `config_identity` match the public profile methods. It does not recompute the schema identity.
- Failed attempts build01 through build06 are kept. build01 did not compile (`E0277` on the digest conversion) and Clippy rejected `while_let_loop`, `collapsible_if`, and `type_complexity`. build03 did not compile (`E0618`, a shadowed `budget` value). The first build01 exec failed with permission denied before `chmod 755`; that attempt is `linux/build01/host-exec-attempt.txt`.
