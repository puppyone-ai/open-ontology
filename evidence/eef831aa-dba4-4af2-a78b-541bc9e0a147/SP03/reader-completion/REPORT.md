# Report — a147 reader full-run admission

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a147`. Slot B. Sole coder. No subagents and no worktree. Slot A `a148` was a disjoint fixed COPY review and was not touched.

This change closes the a146 completeness gap for this owner. It does not sign R4, whole-reader acceptance, converted types, full type support, supply or license approval, or SP03-W03. Capability flags stay false. The ledger was not edited.

## Path spelling

The task named paths that do not exist in this repository. The edit used the existing counterparts:

| Named in the task | Written path |
| --- | --- |
| `crates/oo-connectors/src/postgres/snapshot.rs` | `crates/oo-connectors/src/snapshot.rs` |
| `crates/oo-connectors/src/postgres/stream.rs` | `crates/oo-connectors/src/stream.rs` |
| `crates/oo-connectors/src/postgres/mod.rs` | `crates/oo-connectors/src/lib.rs` public export |
| `contracts/source/postgres-snapshot-lifecycle-v1.md` | `contracts/source/pgsql/reader-lifecycle-v1.md` |
| `tests/pg_reader_complete_live.rs` | `crates/oo-connectors/tests/pg_reader_complete_live.rs` |

`contracts/source/pgsql/bounded-stream-v1.md` was left unchanged. Its line 9 still says `SnapshotOwner::finish` is the run finish and that `SnapshotFinished` is `finish` after a clean close. The lifecycle contract is the admission text for this slice. No file was created at a spelled path that did not already exist.

## Identities

Fixed input `d80472d8a99afd66d8d0e71420f7be2bb62e65c8`, tree `a3b10827acfac28c4abf986831cbcb26823477dc`. Connector production blobs at that commit are identical to `f5f0a73b0678e8b1611f1dc4a40e1dbf1d67b10e`:

- `snapshot.rs` git blob `f1a71fa73064dfb3b83d7d0183f43c6e1a9751a7`
- `stream.rs` git blob `ba9b215a059440a4681578200e7d719cf22407e2`
- `lib.rs` git blob `2f590204e95ddbe26f0ad52d7f58b81124bb1c82`
- `session.rs` git blob `7e191eacbf61439680cbd92a7109ded6c8a05596`, content `6933bf4e855682e6998b89c8c756f8db6239e28e4bc9a249fd34af98c088f16f`
- `reader-lifecycle-v1.md` git blob `4409c11c99f4b3e5f478b94a1176aa75618ec439`
- `bounded-stream-v1.md` git blob `c0f01b52c4a408a7838b607e29322f318f16372e`

`git diff d80472d8 25a844ff -- crates/oo-connectors contracts/source/pgsql` is empty. `session.rs` and `bounded-stream-v1.md` are unchanged at the production commit.

Production commit `16ca2969cd449653a64d98d9831d3c8b8976942a`, tree `0466e01e2794716f1d6bde65ba4ea82d39b88d1b`, parent `25a844ff54bd07e2b7c5e85423acbeedf2b5b5a7`. Message: `connectors: admit a run only after exhausted delivery`. Five paths, 1243 insertions, 14 deletions.

| Path | Git blob | Content sha256 |
| --- | --- | --- |
| `contracts/source/pgsql/reader-lifecycle-v1.md` | `bdae1239de6feaf06f73818f54b784dd0c4e1dbb` | `5beee06fd11370ca973f6066ce7205581b954432df612929176ebc493690a3f6` |
| `crates/oo-connectors/src/lib.rs` | `9f233d68a474f26c81201e1e6e025c8a950e3a06` | `6e4c8cc1afb3293d11a41ee5db3a7de36aca319ee4f6247dbed665cb85387055` |
| `crates/oo-connectors/src/snapshot.rs` | `10176c16a64f45409063a1236c0651065da8a05d` | `43c76114412533600ef554450b50d1f598f04ec674bbffd0611295272dd23227` |
| `crates/oo-connectors/src/stream.rs` | `5deb30e5da09e58904d04cce22760c5ffc284f96` | `7f211507126dea33fd1c20a9b38b29e2e70796437312b89caf23c504d390410f` |
| `crates/oo-connectors/tests/pg_reader_complete_live.rs` | `26ace07d5c2cadcd2584b43d981372834fc29050` | `15b7a7b8e0295044c6a8940988581ca23cf5166c2a604d8e2923b51066619734` |

Probe-3 `expect-*.sha256` matches those five content hashes. The Linux compile used `git archive` of `d80472d8` plus that overlay. Workspace HEAD during smoke and probe-3 was `25a844ff54bd07e2b7c5e85423acbeedf2b5b5a7`.

## What the product does

`SnapshotOwner::finish` still returns `SnapshotFinished` as lifecycle release. That value is explicitly non-admission. `finish_complete` consumes the owner and returns `Result<RunComplete, SourceError>`.

`RunComplete` (`snapshot.rs` 235) is `#[non_exhaustive]` and holds a private `CompleteSeal`. The only constructor in the crate is `mint_complete` (`snapshot.rs` 730). External tests cannot build the token. The token binds `source_id`, `generation`, `config_identity`, `type_policy_identity`, `PgTypePolicy`, `input_digest`, `schema_hash`, the `SourceCatalog` captured at open, and `SelectedTable` values in the owner's admitted order.

The exhausted bit is an `Arc<AtomicBool>` on the owner, cloned into the stream plan. `StreamSlot::next_item` stores it only when `covers_selected` is true and the item delivered to the caller is `StreamItem::Exhausted` (`stream.rs` 462–463). `stream_envelope` recomputes `covers_selected` from the bound tables, empty bounds, and selection order (`snapshot.rs` 1307–1341). A chunk or key-range plan stays false. `TableEof` does not set the bit. Test code has no setter.

`finish_complete` (`snapshot.rs` 666) checks the cancel flag, records a pull error, checks the deadline and `observe_guard` before release, and closes both connections. Early shapes return `protocol` fact `not_exhausted` after that close. Cancellation returns `protocol` fact `cancelled`. A `Budget` pull error is returned ahead of a cleanup error. Catalog, ACL, deadline, poison, and failed release do not mint the token. A token prepared before release is dropped when any of those checks fail.

`connector_capabilities` `scan` and `bounded_read` stay `supported=false` reason `not_in_this_slice` (`model.rs` 735–736). `type_capabilities` `bounded_reader` stays false, reason `not this slice` (`types/mod.rs` 110). Those files were not edited. Probe-3 recorded the three flags false.

## Toolchain and containment

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `HOME=/tmp`. Postgres image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/init.stdout` recorded `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`.

Container `oo-a147-pg` id `1c0ee2944643bb020d0a7f810967c6a3d71e37660e271ebe993f8e59eb6c0b80`, privileged false, network `oo-a147-eef831aa-net`, memory 536870912, `CapDrop=ALL`. Private volumes `oo-a147-eef831aa-cargo`, `oo-a147-eef831aa-target`, `oo-a147-eef831aa-pgdata`, `oo-a147-eef831aa-pglog`. No host network, no docker socket, no host home mount, no shared cargo cache, no auth env, no argv secrets. Passwords and TLS material stayed under `/tmp/oo-a147-eef831aa-dba4-4af2-a78b-541bc9e0a147/secrets`. A byte scan of the 354666-byte Postgres log against those secret files found zero hits. The log is `linux/probe-3/postgresql.log`.

Smoke and probes: `--user 0:0`, `--security-opt no-new-privileges`, `--cap-drop ALL`, `--memory 8g`, `--cpus 4`, `--pids-limit 4096`, private network. Smoke hostconfig `ExitCode=0`, `OOMKilled=false`, `Privileged=false`. Probe-3 hostconfig `ExitCode=0` and the same containment. One own Rust builder at a time. Probe-3 `capacity.txt` records `running_rust_image_or_label=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0` before launch.

The immutable image has no Clippy component. Smoke, probe-2, and probe-3 installed `clippy` for toolchain `1.94.0` into the container writable layer with `HOME=/tmp`. Probe-1 did not, and that is why it exited 1.

## What ran

Smoke finished before any product edit was visible to the compiler. The container mounted a readonly `git archive` of `d80472d8`. `cargo test --locked --offline -p oo-connectors --lib` → `63 passed; 0 failed` (`linux/smoke/lib.exit` 0). `cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings` exit 0. Vendored `sqlx-postgres` printed two unfulfilled `dead_code` expectations. They did not fail the package lint. Elapsed 39s. `linux/smoke/docker-run.exit` 0. Container `oo-a147-smoke` id `21a6e1cdb2f0`.

Probe-1 (`oo-a147-probe` `133f0a6c8acd`) exit 1. `archive_hashes_match` was recorded as `overlay_hashes_match=1`. Clippy was absent: `error: 'cargo-clippy' is not installed for the toolchain '1.94.0-aarch64-unknown-linux-gnu'`. `linux/probe/docker-run.exit` 1, `clippy.exit` 1, `overlay.exit` 0, `toolchain.exit` 0. Elapsed 0s inside the phase. Preserved.

Probe-2 (`oo-a147-probe-2` `d4a123080f5f`) installed Clippy (`clippy-component.exit` 0) and then rejected the integration test. `error[E0308]` at `pg_reader_complete_live.rs:488` passed `SourceError` where `error_label` expects `&SourceError`. `unused_assignments` at line 408, `let mut first_eof = None`. `clippy.exit` 101, `docker-run.exit` 101. The library crate reached the check. Preserved. The test was corrected after this run. Probe-2 was not overwritten.

Probe-3 (`oo-a147-probe-3` `532f73df45bf`) is the green run of the committed overlay. `clippy.exit` 0. `pg_reader_complete_live` `test result: ok. 1 passed` in 2.67s, exit 0. `pg_reader_finish_live` `test result: ok. 1 passed` in 1.23s, exit 0. `docker-run.exit` 0. Elapsed 12s. `overlay.exit` 0, `toolchain.exit` 0. The same vendored `dead_code` expectations printed and did not fail the build.

Commands were `cargo test` and `cargo clippy` with `--locked --offline` and current dependencies. No `cargo update`. No security campaign and no full matrix.

## Live result (`linux/probe-3/complete-outcome.txt`)

The absence clock in this test starts after `finish` or `finish_complete` returns. Each sample calls `pg_stat_clear_snapshot` inside a fresh admin transaction. `CLEANUP_OBSERVATION_BOUND_MS` is 5000.

- Flags: `scan` false, `bounded_read` false, `bounded_reader` false.
- Full drain of `r4.item` and `r4.line`, `batch_rows=1`: generation 1, batches 5, item rows 2, line rows 3, `exhausted=1`, `table_eof=item,line`, `same_cut_match=1` against the pre-writer oracle compared by key. Writer had committed `pine` and line 13 during the open snapshot.
- `finish_complete` returned `RunComplete` generation 1, `source_id=complete_src`, tables `r4.item,r4.line`, `schema_hash=b76a05b20baf5847f2acd640290d33b6765c8d4ce2099e1d0cec0f328b3616db`, `digest=9062f567c847ff02b07d7aabfa773cf29d9123bc831efcfe0ce88c0643883659`. Both owned backends absent at the first sample (`absent_ms=0`, `still=0`).
- Fresh generation 2 read `pine`, 4 lines, `fresh_has_13=1`, scan order `2:oak,1:pine`, `cut_compared_by_key=1`, `old_generation_reused=0`, and its own `RunComplete`.
- Lifecycle `finish` with no stream: `SnapshotFinished` generation 3, `admission=0`, `close_only=1`, backends absent.
- Early idle, before poll, after one batch (ids `1,2` of 4), and one of two tables (`first_eof=rows`, `unread=extra`, `rows_of_first=4`): each `finish_complete=not_exhausted`, `exhausted_seen=0`, `backends_gone=2`.
- Cancel: `finish_complete=Err fact=cancelled`, `run_complete=0`.
- Portal `max_value` on `r4e.wide`: `next_fact=max_value`, `finish_complete=Err kind=Budget fact=max_value`, `run_complete=0`.
- Reader SQL: `oo_statements=397`, `dml_ddl_commit_terminate_offset=0`, verbs `RELEASE=6,ROLLBACK=23,SAVEPOINT=7,SELECT=343,START=18`.

`linux/probe-3/finish-outcome.txt` ends `finish_live_complete`. Drained `r3f.rows` still returns `SnapshotFinished` generation 1, ids `1,2,3`. Fresh generation 2 reads the same ids. Cancel returns `cancelled` with `snapshot_finished=0`. Bound portal returns `max_value` on next and on finish, `snapshot_finished=0`, `reader_rollback_lines=0`, `reader_reset_lines=1`. Reader SQL `oo_statements=161`, `dml_ddl_commit_terminate=0`.

Outcome file ends `complete_live_ok`.

## Left unsigned

R4 has not reviewed this commit. `scan`, `bounded_read`, and `bounded_reader` stay false until that review. `bounded-stream-v1.md` still describes `finish` as the run finish. No production loader admits `RunComplete` yet. The lifecycle text says only `RunComplete` may admit an eventual loader, and `SnapshotFinished` cannot.
