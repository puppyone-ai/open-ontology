# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a222`. Product commit `ed661809cf6c1e8df37af5842838dd1eb839c98a`. The green proof is `linux/proof/`. Attempts 001–006 are unchanged under `linux/proof/attempts/`.

## F1. The column vector needed a type before its first use

Attempt 001 compiled `parse.rs` until `columns.iter()` and stopped with `E0282`. The vector is `Vec<Binding>`. Rustfmt also failed that run. The formatted tree and the type are in the committed `parse.rs`. Attempt 002 got past both. `fmt.exit` and the later test exit on the green run are 0.

## F2. `ConfigError::at` stays crate-private

Attempt 002 called `ConfigError::at` from `tests/adapt.rs` and rustc reported `E0624`. The integration test now goes through `run` and checks that a type refusal does not echo the cell text. `at` was not made public. The green adapter tests exited 0.

## F3. The libclang log directory has to exist before the redirect

Attempt 003 passed the adapter gate, fetch, and tool builds, then `/inside.sh` exited 1 on `bash /extract-libclang.sh > /evidence/libclang/extract.log`. The shell opens the log before the extract script creates the directory. `inside.sh` now runs `mkdir -p /evidence/libclang` first. The green libclang and libclang-load exits are 0.

## F4. Cargo 1.94 writes the Executable line to stderr

Attempt 004 built `query-pg` (`norun.exit` 0) and left `norun.stdout` empty. The line `Executable tests/query_pg_cli.rs (/target/workspace/debug/deps/query_pg_cli-add98238182e5ce1)` is in `norun.stderr`. The script now reads both streams. The green `tee.exit` is 0.

## F5. The two-turn stdout starts with the count row

Attempt 005 saved real stdout, passed the secret scan, and failed the proof with `query-result-adapter: type /1/cells/0`. The 999-byte capture is count turn 0 and then peek turn 1. Binding the peek header type-checks the int cell before the trailer summary. The proof binds that capture with `fixtures/count-header.jsonl`. The extra row after the first summary refuses as protocol. The green proof exit is 0.

## F6. A later `--no-run` can hardlink the binary onto the saved inode

Attempt 006 stopped at `mv: '/target/workspace/debug/query-pg' and '/target/workspace/query-pg.real' are the same file`. Cargo hardlinks `debug/query-pg` to the deps artifact. After the first wrap, the next `--no-run` restored that link onto the inode already named `query-pg.real`. The script copies the ELF aside and then installs `tee.sh` when the debug name is not already the wrapper. The green live test exited 0 in 2.21s.

## F7. Decimal is a binding, and the live fixture has no decimal column

`query-pg` cells in the saved stdout are int, bool, text, and null. Count is the int string `3`. Stamp uses the canonical minimum and maximum int strings. Decimal behavior is covered by `tests/adapt.rs` on handwritten JSONL, including the refusal of `-0.00`. The gateway binary was not given a decimal kind.

## F8. Completion follows status and `end_audit_ok`

The truncated capture is status `succeeded`, `truncated` true, `end_audit_ok` true, one row. The adapter emits completion `complete` and the comparator mismatches the three-row handwritten desk gold. `end_audit_ok` false refuses. Status `partial` stays `partial`. A missing summary refuses as protocol. The proof test covers those four cases and exited 0.

## F9. This proof is not acceptance of B-D02

The comparator, the gateway, and the root lock are the fixed archive plus an unchanged product tree. Gold files are handwritten and are not produced by the adapter. No question bank was run.
