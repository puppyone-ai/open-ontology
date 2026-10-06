# CDC-D03 / SP04-W04 cdc-quarantine

Session `eef831aa-dba4-4af2-a78b-541bc9e0a294`. One Grok 4.7 worker. No subagents and no worktrees.

The crate `tools/cdc-quarantine` stores one normalized control event for `schema_changed` or `truncate`. The contract name is `cdc-quarantine-v1`. A durable result sets `local_prerequisite`. `ack_eligible`, `ack_allowed`, `schema_applied`, `target_truncated`, and `cross_table_consistent` stay false. `target_applied` stays null. Upstream ACK stays false. This unit does not integrate a fenced writer or a control offset.

The fixed input archive is `c8588faab992af1193ff58dbc489a0b117796aa0`, tree `f63d5ce2dcfbc8de759510280aa3761acf0b3e15`. The working HEAD recorded at the overlay launch is `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`. The build used `git archive` of the fixed commit. It did not compile the dirty `tools/cdc-transaction` tree or the untracked `tools/cdc-spool` tree.

## Recipe

Origin file hashes are in `recipe-origin.txt`.

- a204 commit `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, preoverlay launch `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`, preoverlay inside `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636`, overlay-3 launch `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee`, manifest launch `9b28b43d9a9afe9cad69a5f086d7d96af4758f58c7e053a25efa52567a69cdd8`.
- a199 `sha256_manifest.rs` `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`, copied byte-identical.
- a200 commit `a587031c719a484830d78e493605f48e2056cae4` is a PostgreSQL and TLS recipe. It was not applied. This task used zero PostgreSQL.

Deltas from the a204 launch: memory 6 GiB (`6442450944`), `--cpus 1`, `CARGO_BUILD_JOBS=1`, global rust-builder cap 10, owner `a294`, private volumes `oo-a294-eef831aa-cargo` and `oo-a294-eef831aa-target`, network `oo-a294-eef831aa-net` (bridge, internal false). Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `--pull never`. `RUSTUP_TOOLCHAIN=1.94.0`. The repo `rust-toolchain.toml` at the fixed commit says channel `1.98.1`. The environment override selected 1.94.0, recorded in both consoles.

## Preoverlay

`linux/preoverlay/docker-run.exit` is 0. `version.exit`, `blob-cmp.exit`, `overlay-absent.exit`, and `hash-compare.exit` are 0. Offline fetch exited 101. Online locked fetch exited 0. `cargo test --locked --offline -- --test-threads=1` on `tools/cdc-transaction` exited 0: 5 + 3 + 19 tests passed across the suites in `test.stdout`. Clippy of that foreign crate was skipped (`clippy.exit` = `skipped_foreign_crate`). Host config: `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, network `oo-a294-eef831aa-net`. Container id `a1c8b1450d7219f5fadfddc6db8e0241632244cebf863204a5f7396eaee37f2b`.

`tools/cdc-quarantine` was absent from that archive.

## Overlay

`linux/overlay` and `linux/overlay-2` failed to compile. Their consoles and stderr are preserved. `linux/overlay-3` is the passing run.

`linux/overlay-3/docker-run.exit`, `test.exit`, and `clippy.exit` are 0. Offline fetch exited 0. Rustc release is 1.94.0. The unit test `sha256::tests::known_answers` passed. The integration crate `tests/proof.rs` passed 14 tests in 0.17s, including `sigkill_checkpoints_stay_incomplete`, `altered_hash_and_missing_manifest_are_not_repaired`, `exact_replay_does_not_rewrite_bytes`, `payload_conflict_keeps_stored_bytes`, `quota_pause_does_not_persist_the_refused_event`, and `symlink_secret_dotdot_and_foreign_names_are_refused`. `cargo clippy --locked --offline --all-targets -- -D warnings` exited 0. Container id `38ff9b773815d7c4c79c31d753b5baf7e0f642427c7e74b09a78fbf259a31752`. Same 6 GiB and 1 CPU host config.

The SIGKILL tests call `Child::kill`, which sends SIGKILL on Unix. Each stage writes `waiting\n` to a mark outside the store and the parent kills the child. Assertions cover which names exist and that `recover` stays exit 3, paused, incomplete, with `durable_quarantined` false and `ack_eligible` false. They do not claim a torn-byte crash proof. The kernel may flush pages after SIGKILL.

## Pins

`src/sha256.rs` matches the fixed-archive blob. SHA-256 `eac61aa8d9d0eebe935be0d438b6877b40845656601c4c5a8180edd97963f037`.

`Cargo.lock` is the fixed `cdc-transaction` lock with the package name changed once to `cdc-quarantine`. SHA-256 `d701592871d6430df093ca865cf796f83131e51673b1ffa0dcc0a0bc309679df`. Original lock `4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a`. No `cargo update` and no new dependency. Root `Cargo.lock` was not edited.

## Inventory and cleanup

`tools/sha256_manifest.rs` is the a199 tool. The final `MANIFEST.sha256` in this directory is produced by `linux/manifest-close` after the proof logs and the earlier manifest passes. `source/product.sha256` lists the 13 crate files. Empty and `abc` digests in the manifest console match the known answers.

`cleanup.sh` removed only `oo-a294-eef831aa-cargo`, `oo-a294-eef831aa-target`, `oo-a294-eef831aa-net`, and `/tmp/oo-a294-eef831aa-dba4-4af2-a78b-541bc9e0a294`. It did not prune. Foreign Postgres containers were still present afterward (`oo-a295-pg`, `oo-a297-pg`).

This report does not accept the parent denominator, does not close CDC-D03 in the ledger, and does not claim a release.
