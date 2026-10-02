# a108 — versioned target preflight review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a108`. Scoped decision: **approve** the `target-fence-receipts/3` preflight on commit `6cd955af38ce426bb1102a6e078e8580febed776`.

A099-1 is closed on this reviewer's PostgreSQL 17.10 probe of that commit. A missing schema, a missing `_oo.apply_receipts`, and a missing `apply_receipts_segment_check` each raise `OO002` with the message `target-fence-receipts/1 segment relation is absent`, and no version 3 marker is stored. Running frozen `0002` and then `0003` on a database that has no receipt relation still raises `42P01`. That order is recorded as a dead path.

SP04-W05, SP04-W05-a, AT-PL-05, the Loader, CDC, the control-database mirror, stream acknowledgement, physical mapping, 100 workers, a second architecture, runsc, the customer gate, protected CI, full supply-chain, and release stay unsigned. A085-F2, A085-F3, and A085-F5 stay open. Plan checkboxes were not edited.

## Binding

Coordinator pins, confirmed with `git rev-parse` in this session:

| | |
| --- | --- |
| commit | `6cd955af38ce426bb1102a6e078e8580febed776` |
| tree | `463bbf6ada8ea58e676370c872bfd711e9b9c251` |
| parent | `3ca5357930a2c44c42d3061db5fabe577881269a` |
| subject | `pipeline: add target-fence-receipts/3 preflight` |
| author | Redux0223, 2026-10-02 21:45:15 +0800 |

The a104 report omits the delivery SHA on purpose. This review uses the coordinator pin. `e84efd436dbc105b6a80df62464e1af7534e5af7` is an ancestor of the parent. `9a71efaf9cb63a2d63061a241f6b8ee8b00426df` is an ancestor of `6cd955`. Workspace HEAD at review time was `647387de108a8f45c82df2d5387302f8d9f9a899`. The acceptance tree is `git archive` of `6cd955`, mounted at `/src`. It is neither the working tree nor the author's e84efd overlay.

`git diff --stat 6cd955` for `0000`, `0001`, `0002`, `0003`, `segment_preflight_correction.rs`, and `target-fence-receipts-v3.json` was empty. This review did not stage, commit, or edit product, manifest, lock, policy, spec, plan, or older evidence.

## What ran

Deciding run `06f2ff6d-9e03-40b3-bd34-f4ccf1b9642a`.

- Linux arm64, Debian 13, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `RUSTUP_TOOLCHAIN=1.94.0`
- Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- PostgreSQL `17.10 (Debian 17.10-1.pgdg12+1)` on aarch64, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`
- Capacity before the run: `builder_containers=0 host_cargo=0 own_postgres=0`
- One owned builder and one owned PostgreSQL fixture. Trust auth inside that private network. The probe has no password, no `unsafe`, and no C or Python harness.

The independent probe is `probe/src/main.rs`. It speaks the simple query protocol and hashes `0003` itself. Its `apply` is a review oracle. The production tree (`crates/oo-pipeline/src/lib.rs` and `src/bin/oo-pipeline.rs`) does not call `apply_reference` or `segment_preflight_correction`. The executable reference applicator in the repository is the test module `route` / `apply_reference`. That test is not a Loader and not a production installer.

Package commands on the `6cd955` archive, lockfile included:

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test segment_preflight_correction live_segment_preflight -- --ignored --exact
```

Deciding exits: probe 0, fetch 0, package test 0, clippy 0, author live 0, docker run 0, worst 0. The live line is `ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 4.64s`. The author's earlier 4.70s result on the e84efd overlay remains delivery evidence. This run is the committed-tree result.

## Oracles

`pg/summary.log` for run `06f2ff6d` has 154 `PASS` lines and no `FAIL` line. `NOTE fails=0`.

Raw SQLSTATE samples:

- `pg/cases/empty.stderr`: `OO002: target-fence-receipts/1 segment relation is absent`. `to_regnamespace('_oo')` is null.
- `pg/cases/dead-path.stderr`: `42P01: relation "_oo.apply_receipts" does not exist`. Summary marks `dead_path=RED`.
- `pg/cases/missing-relation.stderr` and `pg/cases/absent-constraint.stderr`: the same `OO002` message. Version 3 count stays 0. Dropped constraint count stays 0. `schema_ready` of the v1 hash stays true on the missing-relation database.
- Fresh `0000` + `0001` + `0003`: route `SupersedeFromV1`, one v3 marker, no v2 marker. Flags `true|false|true|false|true|true` are `schema_ready(v1)`, `schema_ready(v3 file hash)`, `segment_preflight_ready(file)`, `segment_preflight_ready(lie)`, view `schema_version_present`, and `segment_contract_ready(text)` absent.
- Lawful v1 receipts stay byte-equal across `0003`. The v1 marker line, including `applied_at`, stays. `segment_preflight_ready(file)` is true.
- Historical v2, built by running frozen `0002` only after the relation exists: receipts stay equal, `segment_contract_ready` definition stays equal, v1 and v2 marker lines stay, and the v3 row stores `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3`. Ready flags `true|true|true|false`: `segment_contract_ready(v2 hash)`, `segment_preflight_ready(v3 hash)`, `schema_ready(v1)`, `segment_preflight_ready(v2 hash)`.
- A v2 row stored as 64 `e` characters raises `OO001` `migration checksum mismatch`. The row and the v1 CHECK stay. `0003` is not executed.
- A v3 row stored as that lie raises the same `OO001`. The row stays. The CHECK stays the v1 form.
- Illegal history raises `OO021` with the fixed message. Receipt text and the v1 CHECK stay. No v3 marker.
- Same-checksum repeat is route `Skip`. The v3 `applied_at` and constraint oid `17774` stay.
- Forced `OO003` `forced migration failure` rolls back. Receipts, the v1 CHECK, and the absent v3 marker stay.
- Advisory wait is `55P03` at 2003 ms. Share-lock wait is `55P03` at 2011 ms. Both are under 8 seconds. No v3 marker is written on the advisory failure.
- Stored lie after a real `0003` execution: `segment_preflight_ready(lie)` is true and `segment_preflight_ready(file)` is false. The v1 checksum stays `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9`. The function does not read the file. Version 3 is authorized only when the caller's own SHA-256 of `0003` is the digest for which `segment_preflight_ready` returns true.
- Foreign owner of `commit_apply_receipt` raises `42501` `target segment preflight refuses to adopt commit_apply_receipt`. The owner stays `postgres`. The CHECK stays the v1 form. No v3 marker.
- Three NULL mixtures on generation 1 raise `23514` and the message contains `closed segment metadata`. Stale illegal shape counts `0|2`: the illegal call is `23514` and the later legal call is `OO013`. Absent-legal, present-legal, and an 8192-byte bitmap insert. An 8193-byte bitmap raises `23514`. Direct unknown insert raises `23514` on `apply_receipts_segment_check` and stores 0 rows.
- Replay on the same backend returns `replay`. Payload, generation, and `target_commit_at` stay `eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee~1~2026-10-02 18:51:24.073134+00`. Conflict is `OO010: apply receipt identity conflict`. The side insert count is 0.
- Search-path poison still stores year `2026`. `segment_preflight_ready` is invoker with `search_path=pg_catalog, _oo`. Runtime insert is `42501`. `oo_pipeline_runtime` is not a member of `oo_target_owner`. The readiness view has `security_invoker=true`.
- Owners after v2 upgrade: `commit_apply_receipt`, `segment_contract_ready`, and `segment_preflight_ready` are `oo_target_owner`.

Static checks on the archive bytes: `0003` SHA-256 is `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3`. `0000`, `0001`, `0002`, v1 JSON, and v2 JSON match the frozen digests. `0003` has no `::regclass` token and three copies of the `OO002` message. It has no `DELETE`/`UPDATE` of `apply_receipts` and no `GRANT oo_target_owner TO oo_pipeline_runtime`. The function, history `DO`, and CHECK slices equal `0002`. The Rust grid count is 17 legal and 163 illegal. The summary labels that count `static_grid_is_not_a_new_pg_180_run`. The 320-case matrix was not rerun. a099's PostgreSQL proof of the 180-cell grid against `0002` is left as that review's evidence.

## Author retention gap

The a104 HANDOFF says the first fixed launch exited 101 after 40.49s, and that the rerun replaced `console.log`, `summary.log`, `live-console.log`, and `cases/`. A search of the a104 evidence, outside `REPORT.md` and `HANDOFF.md`, found no retained first-failure stdout for the four Clippy sites or the `42883` live failure. This review does not reconstruct that stdout from the narrative and does not claim every failed author attempt was retained. The second author run, `bfb4014f-6724-4b46-9359-0d98a0586b64`, is the raw author evidence that remains.

## Overlay and lockfile

`archive/input-hashes.txt` compares the surviving author snapshot `/tmp/oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104/fixed-snapshot` with the `6cd955` archive. These five owned inputs compared equal:

- `0003` `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3`
- migrations README `1b0ea04f37e56ec05c25dd863927568203c47a370116881639ca9e8735a60922`
- `segment_preflight_correction.rs` `6ff6adeeeee6866acf0357b8906bfef2f0039991057c6ca6fbead8e340a7c971`
- `target-fence-receipts-v3.json` `344b45bcdc57b736cfe394f92038922c9944f9695f3346b54ad04ac5c184ba3a`
- contracts README `605fc7aec580831a5f5e467d60e9547861d46b568a3123ce5f8cd8168ceefae3`

The author's published `archive.tar.sha256` is the pre-overlay archive of `e84efd`. `Cargo.lock` changes between that baseline and `6cd955` come from ancestor commits. This review's `cargo test` and `cargo clippy` used the `6cd955` lock.

## This review's earlier runs

Each raw console and phase directory is under `linux/attempts/`. They are harness failures. They are not product verdicts.

| Run | Result |
| --- | --- |
| `76dbc1ff-c7d7-45b8-9904-e76f73022da3` | `rustc` exit 1, `E0382` in `seed_lawful`. Probe did not start. PostgreSQL was ready and was removed. |
| `d126c4e3-b277-4280-abfe-71cd975599d1` | Probe binary on the noexec `/tmp` tmpfs exited 126. Fetch, package test, clippy, and the author live test on a clean cluster exited 0. Live finished in 4.81s. |
| `116fb4b9-2d5c-4efa-a44c-fcf29425ca8d` | Probe exit 1. Replay and conflict opened a new backend, so both returned `OO011` `this backend does not hold the target lock`. The author live test then exited 101 with `42710` `role "oo_target_owner" already exists`. |
| `06f2ff6d-9e03-40b3-bd34-f4ccf1b9642a` | Deciding run. Probe and live test both exit 0. |

## Residuals

A085-F2 stays open. The owner-and-writer insert in this probe is the illegal NULL mixture, and the CHECK rejects it. A lawful owner insert with the writer GUC was not the question under test.

A085-F3 stays open. Conflict rollback covers one side insert in the receipt transaction. A later user-table write with no receipt is outside this slice.

A085-F5 stays open. `owner_token` was not retested as an external worker identity.

A085-F4 stays an installer obligation. `segment_preflight_ready` compares the stored digest. The probe's conjunction held. The function does not close applicator truth by itself.

## Cleanup

Run `06f2ff6d` removed `oo-a108-msrv-06f2ff6d-9e03-40b3-bd34-f4ccf1b9642a`, `oo-a108-pg-06f2ff6d-9e03-40b3-bd34-f4ccf1b9642a`, its network, and its PostgreSQL volume. `docker ps -a` filtered by `oo.owner=a108` and `oo.owner=a104` printed no containers. Retained volumes are `oo-a108-eef831aa-dba4-4af2-a78b-541bc9e0a108-cargo` and `oo-a108-eef831aa-dba4-4af2-a78b-541bc9e0a108-target`.

The two a104 cargo and target volume names from the a104 HANDOFF were absent from `docker volume ls` before the second a108 launch. This review did not run `docker volume rm` on an a104 name and did not delete `/tmp/oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104`.

Stop. No push, release, or customer access.
