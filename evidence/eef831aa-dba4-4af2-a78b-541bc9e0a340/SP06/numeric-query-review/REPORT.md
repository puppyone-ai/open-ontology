# Numeric query review

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a340`. Independent review of child E46 / M1-D14-N on fixed commit `21cc96241a841b47dbbcb116549030e38a643ba0`.

Verdict for this child: **APPROVE_LIMITED**.

`linux/proof/docker-run.exit` is 1 and `linux/proof/proof.exit` is 1. `phases/gold.exit` is 1. The live test had already finished: `phases/test-live.exit` is 0, `test-live.log` is `a327_live::live_numeric_proof` 1 passed in 2.13s, and `outcome.txt` is `numeric_query_recorded`. The first gold oracle wrote `gold_match=0` in `gold-attempt-1.txt`. The corrected oracle wrote `gold_match=1` in `gold.txt` and `gold-rerun.txt`. `phases/gold-rerun.exit` is 0.

The author packet's word Approved, and the product SHA, are the author's record. This verdict is the independent one.

Parent M1-D14, M1-I01, M1-I02, the a317 100k fixture, the CSV decimal integration, and release remain open. `outcome.json` records `parent_acceptance` false, `m1_i01` not_claimed, `m1_i02` not_claimed, and `release` not_claimed. This review adds no unit count and no rework count. This approval is the numeric gate for those unchanged fixtures. This review did not rerun them.

## Fixed tree

Archive commit `21cc96241a841b47dbbcb116549030e38a643ba0`. `overlay.txt` is `none`. `cargo-update-invoked.txt` is `no`. The mount is `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, and `tools/gateway-token-store` from that commit. `oo-gateway` `build.rs` hashes the token-store tree, so the archive includes it. The product tree and the workspace lock were not written.

`rust-toolchain.toml` in the commit sets channel `1.98.1`. `Cargo.toml` sets `rust-version` `1.94.0`. The builder set `RUSTUP_TOOLCHAIN=1.94.0`. `rustc-version.txt` is `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

`linux/proof/archive-sha256.txt` matches `source/expected.sha256`. A later host `shasum -a 256` of the same eight paths in `.a340-work/archive` matched again. `Cargo.lock` is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. The a327 overlay lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` was not the build lock.

`record` in `inside.sh` exits on the first non-zero phase. Gold attempt 1 returned 1, so `lock-hash.txt` was not written. The lock evidence is the pre-build archive hash, the readonly archive mount, and `cargo-update-invoked.txt`.

## Final proof

Directory `linux/proof`. Builder name `oo-a340-proof`, removed by `launch.sh` after `hostconfig.txt`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. `hostconfig.txt`: ExitCode 1, OOMKilled false, Privileged false, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000, NetworkMode `oo-a340-eef831aa-net`. Launch also set `--user 0:0`, `--security-opt no-new-privileges`, `--memory-swap 6g`, `--pids-limit 4096`, `--pull never`, and `--platform linux/arm64`. `linux/proof/capacity.txt`: `host_cargo=0 rust_builders=0 cap_global=10 cap_own=1`.

Private PostgreSQL `oo-a340-pg` id `9bb4a1b0693029a35d7235e656596cc4e1855376fd6d80c3e35035dd65fb5717`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64. Server `17.10 (Debian 17.10-1.pgdg12+1)`. Memory 1073741824, NanoCpus 1000000000, privileged false, network `oo-a340-eef831aa-net`, alias `oo-a340-pg`. Published ports `{}`. Capability drop `ALL`, with `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, and `SETUID` added back. TLS SAN `DNS:oo-a340-pg`. `pg/up.exit` is 0. `pg/up-note.txt` records the first `psql` role miss and the in-place `-U oo_fixture_admin` correction. The cluster was not recreated.

Pre-live phases on the archive, all exit 0:

| phase | exit | result |
| --- | --- | --- |
| toolchain | 0 | rustc 1.94.0 |
| archive-hash | 0 | eight paths match `source/expected.sha256` |
| libclang / libclang-load | 0 | list in `linux/libclang-debs.txt` |
| clippy-component | 0 | |
| fetch | 0 | `--locked`, offline after fetch |
| test-lib | 0 | 43 passed, 0 failed, 0.01s |
| test-query-pg | 0 | `decimal_json_is_exact_and_existing_kinds_stay` |
| test-row-filter | 0 | `absent_row_filter_null_is_optional` |
| clippy | 0 | `cargo clippy -p oo-gateway --lib --bins -- -D warnings` |
| build-pipeline / build-builder / build-gateway | 0 | |
| test-live | 0 | `a327_live::live_numeric_proof` 1 passed, 2.13s |
| gold | 1 | first oracle, preserved in `gold-attempt-1.txt` |
| gold-rerun | 0 | corrected oracle, `gold_match=1` |

The `--lib` run is the fixed commit's suite. It includes the token-store and HTTP unit tests that sit in `21cc9624`, plus the numeric tests `bounded_numeric_admits_precision_1_through_38`, `numeric_hand_literals_keep_scale_and_reject_specials`, `numeric_typmod_checks_scale_and_coefficient_length`, `numeric_wire_matches_the_reviewed_vectors`, and `numeric_quantity_is_the_only_new_admission`. Clippy's log is one cached `Finished` line and the phase exit is 0.

Live chain exits:

| step | exit | stderr |
| --- | --- | --- |
| catalog-pg | 0 | empty |
| captured-pg-draft | 0 | empty |
| review-pg-draft | 0 | empty |
| reviewed-effective-oos | 0 | empty |
| role-pack | 0 | empty |
| role-pack-bundle | 0 | empty |
| init-pg | 0 | empty |
| import-pg-not-ready | 1 | `oo-pipeline: target schema is not ready` (40 bytes) |
| import-pg | 0 | empty |
| reviewed-policy | 0 | empty |
| install-column-acl | 0 | empty |
| provision-loose | 7 | `provision-mapping-reader: policy type` (38 bytes) |
| provision-over | 7 | `provision-mapping-reader: policy type` (38 bytes) |
| provision-mapping-reader | 0 | empty |
| analyze-pg | 0 | empty |
| hidden | 6 | `query-pg: guard` (16 bytes) |
| before-analyze | 5 | `query-pg: policy` (17 bytes) |
| amounts | 0 | empty |

`outcome.json`: amounts_rows 6, ssl true, server_version 17.10, generation 27, policy_tables 1, schema_hash `d64749d8fe1c9934a424dab4f296daadbf6f6d9918bab626e160babb3d66a8b0`, config_digest `47280e45c50fdd7d8a306b98f1265321a17465af3c29afe7df74a298b2d4129c`, bundle_sha256 `c48afb208023db28bedf80662b87496096710a829321d64f966294a2c23d6399`, pack_digest `2afc584343610dbdfdf01be7b5c567e0f607fb4d2f15cffc1438a564c0602bf5`, mapping_grants_before_refusal absent, mapping_grants_after_refusal absent, sql_in_query_output false. schema_hash, bundle_sha256, and pack_digest match author live-4. config_digest on author live-4 is `e51342d9f0fd173507b70469d999e7b5beeb2b045ff9be3f0a3c9f46aaac86b4`.

The amounts audit has two records, protocol `oo.gateway.query-pg-audit/v1`, role `oo_r_clerk`, project `a327num`, policy_id equal to this run's config_digest. Admit and complete both carry original_sha256 `466f22e579afe5252e78ae6cffca1da0a9c4630ba8cb63440a0fc9b7a9c6957a` and executed_sha256 `f03136a329352cbec8ec06fe74cbac7147b26223b25812b17e94501570819326`. Complete status is succeeded, truncated false, returned_rows 6, result_bytes 162, rollback_confirmed true, stop null, sqlstate null. Hidden and before-analyze wrote no audit file. No step stdout contains the redaction sentinel.

## Actual rows

`pg/capture.sh` read `oo_a327_src` with `psql -U oo_fixture_admin` after the proof. `pg/actual/capture.exit` is 0. Source columns are `entry_id`, `amount::text`, `wide::text`, `hidden::text`, `loose IS NULL`, `over IS NULL`.

| entry_id | amount text | wide text | hidden | loose null | over null |
| --- | --- | --- | --- | --- | --- |
| 1 | 12345678901234.5678 | 1234567890123456789012345678901234.5678 | 1 | t | t |
| 2 | -12345678901234.5678 | -1234567890123456789012345678901234.5678 | 2 | t | t |
| 3 | 0.0001 | 0.0001 | 3 | t | t |
| 4 | -0.0001 | -0.0001 | 4 | t | t |
| 5 | 0.0000 | 0.0000 | 5 | t | t |
| 6 | empty | empty | 6 | t | t |

Source `shop.entry` types from `pg_attribute`: entry_id int8 b typmod -1 oid 20; amount numeric b 1179656 oid 1700; wide numeric b 2490376 oid 1700; hidden int8 b -1 oid 20; loose numeric b -1 oid 1700; over numeric b 2555912 oid 1700. `pg_catalog` on the target lists numeric oid 1700, bool 16, int2 21, int4 23, int8 20, text 25, float4 700, float8 701. The float rows are catalog types. They are not `shop.entry` column types.

`amounts.stdout` returned six row records and one summary. Scale is 4 on every decimal cell.

| entry_id | amount | wide |
| --- | --- | --- |
| 1 | coefficient 123456789012345678, negative false | coefficient 12345678901234567890123456789012345678, negative false |
| 2 | same coefficient, negative true | same coefficient, negative true |
| 3 | coefficient 1, negative false | coefficient 1, negative false |
| 4 | coefficient 1, negative true | coefficient 1, negative true |
| 5 | coefficient 0, negative false | coefficient 0, negative false |
| 6 | kind null | kind null |

Summary: succeeded, truncated false, returned_rows 6, result_bytes 162, rollback_confirmed true, original_sha256 `466f22e579afe5252e78ae6cffca1da0a9c4630ba8cb63440a0fc9b7a9c6957a`, executed_sha256 `f03136a329352cbec8ec06fe74cbac7147b26223b25812b17e94501570819326`, stop null, sqlstate null. Author live-4 used the same original_sha256 and the same row shape. Its executed_sha256 is `b7def2bb9721b0f643ba1e37bd6d4e5767b258d6e2ad4886bb0634a115d8adbc`. executed_sha256 is the hash of the deparsed physical SQL on this cluster. The cell values match. The physical-name hash is local to the mapping.

The amounts request has `row_filter` null and selects `entry_id`, `amount`, and `wide` from `shop.entry` ordered by `entry_id`.

Target numeric columns include the reviewed typmods 1179656 and 2490376, plus loose typmod -1 and over typmod 2555912 on the physical table. `_oo` import tables also have `applied_pos` numeric typmod 2490372. That column is import machinery.

## Preserved attempts

| directory | result |
| --- | --- |
| `linux/proof-failed-1` | docker exit 101, OOMKilled false. Archive omitted `tools/gateway-token-store`. `oo-gateway` `build.rs` panicked on `sha256sum` of `tools/gateway-token-store/src/cache.rs`. |
| `linux/proof-failed-2` | docker exit 101. Preoverlay lib 43, decimal, row filter, and clippy were 0, and the three bin builds were 0. `role-pack-bundle` exited 1 because `/target/a327` did not exist under `CARGO_TARGET_DIR=/target/a340`. |
| `linux/proof` gold phase | live exit 0, gold exit 1. The first oracle treated refusal tests and the `atttypid` join as boundary moves, and it required author live-4's executed_sha256. |

`linux/proof/inside.sh` creates `/target/a327` on the private target volume before the live test. Product source is unchanged. The failed-2 `inside.sh` stays without that `mkdir`.

## Recipe origin

`origin/recipes.txt` records a327 live-4 launch and inside, a327 extract-libclang, a327 `pg-wrap.sh`, a200 `up.sh`, a204 preoverlay launch, a199 manifest launch, and a327 `tools/sha256.rs`, with path and SHA-256. Deltas are a340 names, the `21cc9624` archive including `tools/gateway-token-store`, PostgreSQL 1 GiB, one builder at 6 GiB and 1 CPU, private volumes, and `tools/gold.rs`.

## Scope kept

`grantable_base_type` admits numeric only for precision 1 through 38 with a non-negative typmod. `supported_base` also requires `*oid == attribute.type_oid` and typtype `b`. `ensure_grantable_type` and `ensure_supported_type` call `grantable_base_type`, require typtype `b` and `atttypmod`, and join `pg_type` on `atttypid` to read the type row. Those two functions contain neither `type_oid` nor `types.get`. Gold notes: `catalog_oid_equality=supported_base`, `install_oid_equality=absent`, `mapping_reader_oid_equality=absent`, `type_base=typtype_b`, `domains=refused_by_typtype`.

`COMPARISON_SIGNATURES` contains no numeric entry. `AGGREGATE_STAR_NAME` remains `count`. The archived catalog has no `NUMERIC_LITERAL`. `query-pg` accepts `Value::Null` only for the key `row_filter` and rejects every other null with `Fail::Request`. The decoder names OID 1700, `CANONICAL_MAX_COEFFICIENT_DIGITS`, `numeric_nan`, `numeric_infinity`, and `numeric_neg_infinity`. `fn base_type` contains neither `float4` nor `float8`. `outcome.json` records that numeric(18,4) rejects Infinity at insert with sqlstate 22003, and that NaN import refused `numeric_nan`.

Root Axum triples and the a337 HTTP review stay outside this numeric scope. This review wrote no dependency.

This review makes no full SQL capability claim. Numeric comparisons, numeric literals, and numeric aggregates remain unsupported.
