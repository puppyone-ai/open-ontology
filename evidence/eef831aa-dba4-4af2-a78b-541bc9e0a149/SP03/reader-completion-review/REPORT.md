# Report — a149 reader completion review

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a149`. Slot B. Sole independent review. No subagents and no worktree. Slot A `a150` owns the target installer, including pipeline `Cargo.toml` and the root lock. This review did not read or mount those moving files. Every compile used `git archive` of `16ca2969cd449653a64d98d9831d3c8b8976942a`.

Verdict: **APPROVE_LIMITED**. The PostgreSQL full-mode reader production API at `16ca2969cd449653a64d98d9831d3c8b8976942a` is approved as the R4 close of the a146 admission gap. `SnapshotOwner::finish_complete` returns `RunComplete` only after this owner delivered `StreamItem::Exhausted` for every selected table and the existing release checks passed. `SnapshotOwner::finish` returns `SnapshotFinished` as lifecycle release. That value is non-admission.

This approval is the reader prerequisite a later target-apply decision can cite. It does not check SP03-W03. It does not edit the ledger. It does not flip `scan`, `bounded_read`, or `bounded_reader`. It does not sign full types, converted types, CDC, incremental mode, other databases, the registry API, release, or supply and license approval. The author's a147 run is not this approval.

## Fixed source

Commit `16ca2969cd449653a64d98d9831d3c8b8976942a`, tree `0466e01e2794716f1d6bde65ba4ea82d39b88d1b`, parent `25a844ff54bd07e2b7c5e85423acbeedf2b5b5a7`. Smoke `archive-tree.txt` and probe `archive-tree.txt` both record that tree. `archive-base.txt` records the commit. Workspace HEAD during both launches was `0607293ad17c6c157e054355fdda480c4b587fc8`. The archive did not follow HEAD.

Author evidence is `3df15045` (`evidence: record the a147 reader completion proof`), tree `cd2131bd11ce594be2b50ff50e2ac36d19d4359c`. `SHA256SUMS` in that commit has 112 lines. The author live tests exited 0. This review treated that as the change under test, not as acceptance.

Blobs at `16ca2969`, matched by smoke `fixed-blobs.txt` and probe `fixed-blobs.txt` (`archive_hashes_match=1`, `overlay_hashes_match=1`, `production_files_unchanged_after_probe=1`):

| Path | Git blob | Same as a146 `f5f0a73b` |
| --- | --- | --- |
| `crates/oo-connectors/src/session.rs` | `7e191eacbf61439680cbd92a7109ded6c8a05596` | yes, content `6933bf4e855682e6998b89c8c756f8db6239e28e4bc9a249fd34af98c088f16f` |
| `crates/oo-connectors/src/key_plan.rs` | `8f152763844330532df58efdc77568e1f36e3d40` | yes |
| `crates/oo-connectors/src/model.rs` | `0d7af7c1b9b7ffc30bbecdc75b934c965c8dbefa` | yes |
| `crates/oo-connectors/src/types/mod.rs` | `9920752ceee306796ff5ea4113f5d6109b659555` | yes |
| `crates/oo-connectors/src/sql.rs` | `6badaf187e33346c1657c632e6d5c43fe90f1ea2` | yes |
| `crates/oo-connectors/Cargo.toml` | `e8e3254da87a29ce90c485e8ad6566dbd4654f43` | yes |
| `Cargo.lock` | `15ae1071c7236a956547e3f1d37d627b2cbebfce` | yes |
| `contracts/source/pgsql/bounded-stream-v1.md` | `c0f01b52c4a408a7838b607e29322f318f16372e` | yes |
| `contracts/source/pgsql/reader-keys-v1.md` | `346833019418700f39e3cc3061d74dda4dbcabd8` | yes |
| `contracts/source/pgsql/reader-lifecycle-v1.md` | `bdae1239de6feaf06f73818f54b784dd0c4e1dbb` | changed, content `5beee06fd11370ca973f6066ce7205581b954432df612929176ebc493690a3f6` |
| `crates/oo-connectors/src/lib.rs` | `9f233d68a474f26c81201e1e6e025c8a950e3a06` | changed, content `6e4c8cc1afb3293d11a41ee5db3a7de36aca319ee4f6247dbed665cb85387055` |
| `crates/oo-connectors/src/snapshot.rs` | `10176c16a64f45409063a1236c0651065da8a05d` | changed, content `43c76114412533600ef554450b50d1f598f04ec674bbffd0611295272dd23227` |
| `crates/oo-connectors/src/stream.rs` | `5deb30e5da09e58904d04cce22760c5ffc284f96` | changed, content `7f211507126dea33fd1c20a9b38b29e2e70796437312b89caf23c504d390410f` |

The author test `crates/oo-connectors/tests/pg_reader_complete_live.rs` is in the commit. This review did not run it. The independent overlay is `overlay/pg_reader_r4_review.rs`, content `610d0c219b6d6bd056a700a5e637bae91ad128fa22992935f6e3c30892a88130` (`linux/probe/expect-review-test.sha256`).

## What the production API does

`reader-lifecycle-v1.md` at this commit is the admission text. `finish` returns `SnapshotFinished` as lifecycle release. `finish_complete` returns `RunComplete` only after this owner delivered `StreamItem::Exhausted` for every selected table and cancellation, deadline, current catalog and ACL, and both releases have passed. `TableEof` does not record delivery. A key range or a single-table chunk does not record it. Callers cannot construct `RunComplete`. The four early shapes return `protocol` fact `not_exhausted` after both connections close. Cancellation, a budget or portal or source failure, a deadline, a current catalog or ACL failure, and a failed cleanup return that failure and do not mint the token.

`RunComplete` binds source id, generation, config identity, type-policy identity, `PgTypePolicy`, input digest, schema hash, the catalog captured at open, and the selected tables in admitted order.

## Requirement map

Original card: `execution/prompts/v01-pg-bounded-reader-prepared.md`, step 4. a146 reviewed `f5f0a73b` and returned REQUEST_CHANGES because `finish` returned `SnapshotFinished` for no stream, an unpulled stream, one batch of a four-row table, and one of two selected tables. This order reused that R1/R2/R3 evidence where the blobs are unchanged and required a fresh proof of the admission API. The full security, edge, and dual-architecture matrix was not rerun.

| Requirement | Fixed source | Evidence | Result |
| --- | --- | --- | --- |
| Freeze R1–R3 code and the locked dependencies | Unchanged blobs in the table above, including `Cargo.lock` `15ae1071c7236a956547e3f1d37d627b2cbebfce` | Smoke and probe `fixed-blobs.txt`, `archive_hashes_match=1` | Held. Commands were `--locked`. No `cargo update`. |
| Linux 1.94 connector regression on the frozen archive | `git archive` of `16ca2969` | `linux/smoke/console.log`: lib `63 passed; 0 failed`, clippy phase exit 0, `docker-run.exit` 0, elapsed 40s | Proved on this commit. |
| Two selected tables, one cut taken before a concurrent writer, actual Arrow, then `Exhausted`, then a bound `RunComplete` | `finish_complete` after a covering `open_stream` | `linux/probe/review-outcome.txt` generation 1: batches 5, item rows 2, line rows 3, `arrow_bytes=3404`, fields `r149.item=id,name;r149.line=line_id,item_id,qty`, `exhausted=1`, `same_cut_match=1`, scan `1:willow,2:oak`. Token generation 1, `source_id=r4_review_src`, tables `r149.item,r149.line`, schema hash `b44ae80c153942684999963bca7017de7d009524917200bbb2dba836e27e5dac`, digest `f34ebaca9313b6951ad5ecefe19137a0044a07e5076841d154bd19d2bf7955fd`. Both backends absent at the first sample | Proved. Writer had committed `pine` and line 13 while the snapshot was open. The cut stayed `willow` and 3 lines. |
| A fresh generation reads the changed cut and does not reuse the old generation | New `open` is a new generation | Generation 2: `fresh_item1=pine`, `fresh_lines=4`, `fresh_has_13=1`, scan `2:oak,1:pine`, `cut_compared_by_key=1`, `old_generation_reused=0`, own `RunComplete`. The test asserted the fresh input digest differs from generation 1. The outcome line prints the shared schema hash and does not print the second digest | Proved. |
| Four early cases return `not_exhausted` and both backends are released | `finish_complete` when the exhausted bit is unset | Generations 5–8 in `review-outcome.txt`. Each `backends_gone=2`, `run_complete=0`, `absent_ms=0` | Proved. This is the a146 gap. |
| `finish` is non-admitting, including after `Exhausted` | `finish` returns `SnapshotFinished` | Generation 3 idle: `SnapshotFinished`, `admission=0`, `close_only=1`, pid 149. Generation 4 after `Exhausted`: `SnapshotFinished`, `admission=0`, `run_complete=0`, pid 151 | Proved. |
| Callers cannot construct the token or name the pull plan | `RunComplete` is `non_exhaustive` with a private seal. `mod stream` is private | `linux/probe/private-construct.cargo.exit` 101. Compiler errors E0639, E0308, E0599, E0603, E0423. Harness `private-construct.exit` 0 because that failure was required. `private_construction_rejected=1` | Proved. |
| `TableEof`, a key range, and a chunk do not mint, including when the item is `Exhausted` | Covering scans set the bit. `stream_plan_one` does not | Generation 8: first `TableEof` is `rows`, `extra` unread, `exhausted_seen=0`, `not_exhausted`. Generation 9 key range: `exhausted_item=1`, `covers_admission=0`, `not_exhausted`. Generation 10 chunk ordinal 0: `exhausted_item=1`, `covers_admission=0`, `not_exhausted`. Each `backends_gone=2` | Proved. A drained subset can emit `StreamItem::Exhausted` and still return `not_exhausted`. |
| Cancel, portal failure, deadline, ACL, catalog drift, and cleanup cannot mint | `finish_complete` returns the failure and drops any prepared token | Generation 11 `cancelled`. Generation 12 `Budget` `max_value`. Generation 13 `Budget` `deadline` (first absence sample 3 ms). Generation 14 `Privilege` `select_not_granted`. Generation 15 `Catalog` `schema_drift`. Generation 16 `Protocol` `database` after `admin_terminated_backends=2`. Each `run_complete=0` | Proved for these arms. See the cleanup note below. |
| Reader path does not write, commit, or terminate backends | Private `sql` module, unchanged | `sql_audit oo_statements=724 dml_ddl_commit_terminate_offset=0 verbs=RELEASE=13,ROLLBACK=42,SAVEPOINT=14,SELECT=623,START=32`. Outcome ends `r4_review_ok` | Proved on this run. |
| R1–R3 composition already reviewed on the frozen blobs | a146 `REPORT.md` rows marked re-proved on `f5f0a73b` | Those session, stream-budget, key-plan, sql, and lock blobs are the blobs above | Reused. This session did not rerun that matrix. |
| Capability advertisement | `model.rs` and `types/mod.rs`, unchanged. Lifecycle paragraph 1 still says the flags stay false | Outcome lines: `scan` false `not_in_this_slice`, `bounded_read` false `not_in_this_slice`, `bounded_reader` false `not this slice` | Deliberate. Follow-up activation after this acceptance. Not a functional failure of `finish_complete`. |

Cleanup note. Generation 16 terminated both reader backends after `Exhausted` and called `finish_complete`. The result was `Protocol` fact `database`, `run_complete=0`, backends already gone. That is the driver catch-all for an unmapped database error. The named fact `cleanup_unconfirmed` was not the string returned. A mint would have failed the test. The 5000 ms drain, a failed guard cancel, and the no-runtime `Drop` arm stay where a146 left them: read, not provoked. The shortened deadline was an 8 second open slept past its `Instant`. A wall-clock 300 second run was not started. `SNAPSHOT_DEADLINE_DEFAULT` remains in the unchanged `key_plan.rs`.

REVOKE on `r149a.gate` and ALTER on `r149a.cat` both committed after `Exhausted`. The lock-timeout fallback in the overlay did not run. `finish_complete` still returned `select_not_granted` and `schema_drift`.

## Terminology residual

`bounded-stream-v1.md` blob `c0f01b52c4a408a7838b607e29322f318f16372e` is unchanged. Line 9, copied into `linux/probe/bounded-stream-line9.txt` and into outcome line 2:

`SnapshotOwner::finish` is still the run finish. `TableEof` is the end of one table. `Exhausted` means every selected table has reached `TableEof` and the owner is still open. `SnapshotFinished` is only `finish` after a clean close.

`reader-lifecycle-v1.md` is the newer contract on the same commit. Its opening paragraph says `finish` returns `SnapshotFinished` as lifecycle release, that value is not admission, and `finish_complete` returns `RunComplete` only after `Exhausted` for every selected table. Line 44 says `SnapshotFinished` is the successful lifecycle close and is not admission, and that `RunComplete` is the only value that may admit an eventual loader. The slice still has no production target admission.

`key_plan.rs` lines 67–68, blob `8f152763844330532df58efdc77568e1f36e3d40`, still say the loader treats anything other than `SnapshotFinished` as incomplete. That comment is the same class of residual. A loader that followed the comment would treat `RunComplete` as incomplete. The executable contract is the lifecycle text plus the type of `finish_complete`. Live generation 4 delivered `Exhausted` and `finish` still returned `SnapshotFinished` with `admission=0`. These two texts are documentation follow-ups. They are not a functional failure of the API that was executed.

`lifecycle_names_run_complete=true` was read from the archived lifecycle file inside the probe.

## Toolchain and containment

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, Debian 13. `HOME=/tmp`. The image has no Clippy component. Each container ran `rustup component add clippy --toolchain 1.94.0` into its writable layer. Smoke `clippy-component.exit` 0, then `cargo fetch --locked` (`fetch.exit` 0). Probe reused the private cargo and target volumes with `--offline`.

Postgres image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`. `pg/init.stdout` records `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`. `pg/init.exit` 0. `pg/up.exit` 0. Database `oo_a149`. Roles `oo_a149_reader` and `oo_a149_writer`. TLS verify-full. `pg/server-san.txt`: `CN=oo-a149-pg`, SAN `DNS:oo-a149-pg`. `pg/pg-ports.json` is `{}`.

Container `oo-a149-pg` id `99d2efb3884db660e350c37ea6bcf1e379810618cfb000e459d50515e77d34f2`, privileged false, network `oo-a149-eef831aa-net`, memory 536870912, `CapDrop=ALL` plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, `SETGID`. Private volumes `oo-a149-eef831aa-cargo`, `oo-a149-eef831aa-target`, `oo-a149-eef831aa-pgdata`, `oo-a149-eef831aa-pglog`. No host network, no docker socket, no host home mount, no shared cargo cache, no auth env, no argv secrets. Passwords and TLS private keys stayed under `/tmp/oo-a149-eef831aa-dba4-4af2-a78b-541bc9e0a149/secrets`. A byte scan of this evidence tree against `reader.pass`, `writer.pass`, `super.pass`, `pointer.json`, `ca.key`, and `server.key` found zero hits. `linux/probe/postgresql.log` is 459328 bytes.

Smoke and the probe: `--user 0:0`, `--security-opt no-new-privileges`, `--cap-drop ALL`, `--memory 8g`, `--cpus 4`, `--pids-limit 4096`, private network. Smoke hostconfig `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, id `a6b30c0b5efaf10c96e0eb4cf77d6e17dd36136d13eea132ae96058ca04f0968`. Probe hostconfig the same containment, id `f98559450ed7987703d508e4f1117f7ad9e25814abd4677838ba3c144a3718ed`. Capacity before each launch: `running_rust_image_or_label=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0`. One builder at a time. Postgres stayed up. No host `cargo`, `rustc`, `rustdoc`, or `rustup` process was running at the identity check.

## What ran

```
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a149/SP03/reader-completion-review/pg/up.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a149/SP03/reader-completion-review/linux/smoke/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a149/SP03/reader-completion-review/linux/probe/launch.sh
```

The probe launch was invoked twice. The first container is preserved as `linux/probe-1`. Its container `f7929b4827990c46179798d3127f30c4491e5ffbdad9d86fb622ecd34d3fc941` was removed before the second launch because the script refuses to overwrite `console.log` and refuses a live name collision. The second launch is `linux/probe`.

Smoke, archive only, no overlay:

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

`lib.exit` 0. `clippy.exit` 0. `docker-run.exit` 0. Vendored `sqlx-postgres` printed two unfulfilled `dead_code` expectations. They did not fail the package lint.

Probe-1 (`linux/probe-1/docker-run.exit` 101, `clippy.exit` 101, elapsed 1s). Clippy component install had already exited 0. The overlay failed to compile. Rust 1.94 treats `{generation}` as an implicit argument, and two `format!` calls also passed `generation` positionally, so `one_line(&blocked)` was unused at `pg_reader_r4_review.rs:595` and `:641`. Those branches are the blocked-DDL fallback. They did not run in the green probe. The overlay was corrected. Production source was not edited. `linux/probe-1/expect-review-test.sha256` is `678edf55b992005c26442666168b5e7c0382f6f097cb2a9a5a86e92f483bf418`, the pre-fix overlay. Leave probe-1 in place.

Green probe: clippy exit 0, then

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_r4_review -- --test-threads=1 --nocapture
```

`test result: ok. 1 passed; 0 failed; finished in 12.83s`. `pg_reader_r4_review.exit` 0. `docker-run.exit` 0. Elapsed 18s. Outcome file ends `r4_review_ok`.

Private construction ran after that test, on a copy of `overlay/run_complete_private_fail.rs`. `cargo test` exited 101. The harness required a nonzero cargo status and the private-construction diagnostics, then deleted the fail file and rechecked production hashes. `production_files_unchanged_after_probe=1`.

No production, manifest, lock, spec, or ledger file was edited.

## Left unsigned

SP03-W03 stays open. The ledger stays as it was. `scan`, `bounded_read`, and `bounded_reader` stay false until a later activation. `bounded-stream-v1.md` line 9 and the `PlannedChunk` comment still use the old lifecycle words. No production loader admits `RunComplete` in this slice. Full types, CDC, other databases, incremental mode, release, and supply approval stay unsigned. The named cleanup fact `cleanup_unconfirmed` was not observed.
