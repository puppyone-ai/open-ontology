# Findings

## Compile failures preserved

`linux/overlay` exit 101. `src/model.rs` passed a `String` to `Reason::parse`, which takes `&str`. `read_limited` marked a `File` mutable, and clippy `-D warnings` rejected that. The lib did not compile, so the tests did not run. Container `9fca0313a87dcfa339062aa7634e5c857460f99fa487398e5981e1ff99bca638`.

`linux/overlay-2` exit 101. Two errors:

- `tests/proof.rs` used `env!("CARGO_BIN_EXE_cdc_quarantine")`. Cargo 1.94 set `CARGO_BIN_EXE_cdc-quarantine`. The test now checks that name first, then the underscore form, matching `tools/cdc-transaction/tests/cli.rs`.
- `parse_control` returned `Result<ControlEvent, Rejected>`. `Rejected` owns the payload, and clippy `result_large_err` fired. The return type is now `Result<ControlEvent, Box<Rejected>>`.

`linux/overlay-3` is the first compile that ran the tests. Exit 0.

## Behavior fixed before the first container

These were corrected in the working tree before preoverlay. The Linux run did not observe the earlier versions.

- `Binding::parse` treated epoch with the source-position rule under the wrong boolean precedence, so a normal epoch failed to parse. It now uses `valid_epoch` for a canonical `u32` decimal.
- `admit_new` called `prepare` with `next_events` 0, so a lowered cap of 0 events would not refuse. It now passes 1.
- A digest mismatch or an empty or over-cap payload dropped the parsed event, so the status stayed unpaused. `parse_control` now returns that event inside `Rejected`, and the CLI sets `paused` and `attempt_collection`. The overlay-3 test `digest_and_oversize_pause_without_a_directory` covers this.
- `replay` used to call `match_existing` on a torn tree. It now refuses `foreign_directory` or `incomplete` first. The SIGKILL test then replays and still has no `RECEIPT`.

## Contract file timing

`CONTRACT.md` was written in this session after `model.rs` and `store.rs` already carried the same field list. The schema was not edited after the passing overlay. There is no earlier commit that contains only the contract.

## Proof limits

SIGKILL before fsync shows which names exist. It does not prove that file bytes were torn. `CONTRACT.md` says the kernel may flush pages after the signal. The tests assert partial versus final names, the absence of `RECEIPT`, and a second `recover` that does not seal or delete.

The disk cap is a byte counter of regular files this process has seen. It is not an operating-system quota.

`source_position` is caller-declared decimal text. The sample value `18446744168199581248` is not checked as an LSN.

Recovery unions `paused_collections` from durable manifests and any collection name it can still parse from an incomplete manifest. It does not rewrite an older manifest when a later event arrives.

`linux/manifest-close` writes `docker-run.exit`, `hostconfig.txt`, `console.log`, and `container-removed.txt` after `MANIFEST.sha256` is produced. Those four files are outside that inventory. A later inventory pass, if one is run after this findings file, covers the earlier manifest hostconfigs and these markdown files, and leaves only that later pass's own post-run files outside the hash.

## Left untouched

The ledger, the acceptance count, the specs, the root lock, and `tools/cdc-transaction` were not edited. No connector manifest was edited. a207 remains the lock writer for that surface. No dependency was added.
