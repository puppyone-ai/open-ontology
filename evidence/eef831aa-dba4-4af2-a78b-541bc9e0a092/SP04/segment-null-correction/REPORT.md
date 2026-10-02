# a092 — forward target segment NULL correction

Session `eef831aa-dba4-4af2-a78b-541bc9e0a092`. One forward migration, `0002_target_segment_null_expand.sql`, closes unknown `segment_index` / `segment_count` combinations for `target-fence-receipts/2`. The v1 SQL and the v1 JSON contract are unchanged.

## Identities

Assigned baseline, still an ancestor of the commit parent:

- commit `fc877c482a61729ea46e779a674a46f8f72c80c5`
- tree `d40755c31abd82d932b56886aa048ab4d7aa5e4b`
- parent `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df`

Parent at the moment this report was committed:

- commit `82d4c85492fc1b76fa1048bbd970de2909594295`
- tree `95cc13b92b7d57833c268b15550e8c45c7f3f17f`
- subject `docs: bind reader and apply cards to approved corrected inputs`

The delivery commit is the commit that contains this file. Its parent is the commit above if HEAD had not moved. `git show --name-only` on that commit is the ownership check.

## Migration

SHA-256 of `crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql`:

`59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512`

Unchanged, rechecked against the files and against both JSON contracts:

- `0000_bootstrap_roles.sql` `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208`
- `0001_target_fence_expand.sql` `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9`

`0002` does not insert `_oo.schema_migrations`. The applicator inserts `target-fence-receipts/2` in the same transaction after the file returns. Repeat with the same checksum does not re-execute the file. A different checksum raises `OO001` and leaves the stored row. Missing v1 relation or `apply_receipts_segment_check` raises `OO002`. An existing receipt that is not one of the two closed shapes raises `OO021` with one fixed message and no `DELETE` or `UPDATE` of `apply_receipts`.

Legal shapes are `(NULL, NULL, empty bitmap)` and a non-null index/count with `count >= 1`, `0 <= index < count`, and a bitmap of 1..8192 bytes. The CHECK wraps that predicate in `IS TRUE`. The function uses `IS NOT TRUE` and raises `23514` `closed segment metadata` before `_oo._load_held_generation`. A NULL bitmap is still `23514` `closed completion metadata`. `_oo.schema_ready` stays v1-only. `_oo.segment_contract_ready(text)` compares the caller argument to the stored v2 checksum and does not read the file. A true v1 `_oo.schema_ready` does not authorize a driver that requires the v2 contract.

## Checks

Linux arm64 Rust 1.94 image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` (17.10, linux/arm64). No pull.

Baseline smoke, archive of `fc877c4` only, run `c19c5871-6b3e-4b98-8623-0a820532d71f`:

```
evidence/.../linux/baseline/launch.sh
```

`fetch.exit=0` `test.exit=0` `clippy.exit=0` `docker-run.exit=0` `worst.exit=0`. Host config: `privileged=false`, `capdrop=["ALL"]`, `ports={}`. Cleanup of that container, network, and both volumes exited 0.

Host harness compile used Homebrew `rustc 1.98.1 (48a229cea 2026-09-01)`, because the live test is `#[ignore]` and the 1.94 package smoke does not execute it:

```
rustc --edition 2024 -D warnings --test crates/oo-pipeline/tests/segment_null_correction.rs -o /tmp/oo-a092-segment-null-test
/tmp/oo-a092-segment-null-test published_v1_bytes_stay_and_v2_closes_unknown_segments --exact
/tmp/oo-a092-segment-null-test live_segment_null_correction --ignored --exact --nocapture
```

The offline test passed. The live test failed three times in the harness, then passed in 26.77s. The failures were `GRANT CONNECT ON DATABASE current_database()` (invalid syntax), a SQLSTATE parser that missed the `psql:<stdin>:` prefix, and boolean `::text` printing `true`/`false`. `pg/summary.log` appends those attempts; the case files are the passing run; the last line is `live=pass`. PostgreSQL reported `PostgreSQL 17.10` on aarch64. Container `oo-a092-pg1710-d4e8a092-7c31-4b6e-9f10-a0925e600001`. Host config: `privileged=false`, `ports={}`, cap-drop `ALL`, cap-add `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`, `no-new-privileges`. `pg/cleanup.txt` is `rm_exit=0`, `network_exit=0`, `volume_exit=0`.

Passing fixture notes include: v1 fresh generation 1 returned `inserted` for null index, null count, and both null, each with a one-byte bitmap; forward of that history raised `OO021` and the snapshot stayed equal; v2 ready stayed false; the same shapes on the fixed database raised `23514`; empty, one-byte, 8192-byte, and index 8191/count 8192 inserted; 8193, negative, `index = count`, count 0, mixed nulls, and a NULL bitmap raised `23514`; a stale legal segment on the holding backend raised `OO013`; a stale illegal segment raised `23514`; runtime insert `42501`; bad codec `OO015`; direct illegal insert hit `apply_receipts_segment_check`; side row and adopt rolled back with `23514`; replay left payload, `fence_generation`, and `target_commit_at` unchanged across a one-second sleep and a later generation; conflict `OO010` left the row and dropped the side insert; same-shard adopt `55P03` and a different shard adopted generation 1; migration lock wait was 2087 ms and raised `55P03`; kill before commit left count 0 and retry inserted; kill after commit left count 1 and replay kept the row; catalog saw `IS TRUE`, `search_path=pg_catalog, _oo`, and no runtime membership in `oo_target_owner`. Fresh load rejected both-null plus a byte and left zero receipts. Forced `OO003` restored the v1 function body and left v2 ready false. A stored lie checksum made `segment_contract_ready(lie)` true and `segment_contract_ready(file hash)` false while v1 `schema_ready` stayed true.

That live process was compiled before three later edits: `CARGO_MANIFEST_DIR` for the hash path, returning the docker `Output` directly, and `PathBuf::from` without `format!`. Those edits do not change the SQL or the fixture assertions. A second PostgreSQL container was not started.

Fixed smoke attempt `51b9fcf0-eb99-4b82-a8f8-f914415571cf` (archive plus owned overlay) fetched with exit 0, then `test.exit=101` and `clippy.exit=101`. The test hashed a package-relative path from the workspace `file!()` string. Clippy rejected `let_and_return` and `useless_format`. Evidence is `linux/attempt-51b9fcf0-eb99-4b82-a8f8-f914415571cf/`. Cleanup of that container, network, and both volumes exited 0.

Fixed smoke `2ad2946a-e63e-478f-9397-f2539d27ea56`, fresh `fc877c4` archive plus only the owned overlay:

```
evidence/.../linux/fixed/launch.sh
```

Inside the container the three migration hashes matched the values above. `fetch.exit=0` `test.exit=0` `clippy.exit=0` `clippy-component.exit=0` `docker-run.exit=0` `worst.exit=0`. `cargo test -p oo-pipeline` reported `segment_null_correction` as 1 passed, 1 ignored. Host config: `privileged=false`, `capdrop=["ALL"]`, `ports={}`. Cleanup of that container, network, and both volumes exited 0.

Commands inside both smokes, with no `-p` on fetch:

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
```

## Cleanup and residuals

Exact UUID removals only. No prune. After the last cleanup, `docker ps -a`, `docker network ls`, and `docker volume ls` filtered by `oo.owner=a092` were empty. `/tmp/oo-a092-eef831aa-dba4-4af2-a78b-541bc9e0a092` (baseline snapshot, fixed snapshot, `pg-secrets`) and `/tmp/oo-a092-segment-null-test` were removed. `tmp_residual=no`.

## Not claimed

SP04-W05, AT-PL-05, Loader, CDC, control-database mirror, reject-event durability, stream acknowledgement, publication, physical identifier mapping, 100-worker acceptance, arbitrary SQL fencing, a second architecture, and a release are not claimed. A085-F1 is the defect this migration closes. A085-F2, F3, and F5 stay open. F4 is answered for `target-fence-receipts/2` by the external conjunction; v1 `schema_ready` behavior is unchanged. a088 still owns root and NFC. a093 still owns connector source and tests.
