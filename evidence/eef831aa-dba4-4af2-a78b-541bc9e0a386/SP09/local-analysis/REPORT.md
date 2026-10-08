# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a386`. Plan item SP09-W05.

## Verdict

The card stays incomplete. `closes_sp09_w05` is false in every manifest this run wrote. This card does not accept itself, and it does not accept SP09, AT-API-04, or the parent goal. Codex updates the ledger. The ledger was not edited.

What ran is the `rust_exact` loop: a fresh private PostgreSQL 17.10 query, the frozen `query-pg` and `query-result-export` producers, hash and lineage binding, exact decimal group sums, formula-safe CSV, a native Chinese SVG, a source manifest, cooperative cancel, and refusals for DuckDB, xlsx, and parquet. DuckDB CLI, Arrow, Parquet, an Excel writer, a bundled CJK font file, and a kill of a native child remain unmet. `tools/local-analysis/DEPENDENCY-PROPOSAL.md` is a proposal with `audited` false. It admits nothing.

## Producers

Producer commit `21cc96241a841b47dbbcb116549030e38a643ba0`. Exporter commit `62b4afecdd377fa94310f46467c48a01e61d14db`. Both were `git archive` inputs. Root lock before and after the chain is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`.

Recipe origins are the comments in `pg/up.sh`, `linux/chain/launch.sh`, and `linux/chain/inside.sh`. The a344 chain script SHA-256 cited there is `292a700bb0a323de575e51738f46158f108c41dd676378b0121319e2631171a9`. The PG script origin SHA-256 is `9ddc412a92f5c96dbcaaf8871d284f0a4fcccf5f05be330396b395bd2ab89d78`. This card changed owner labels to `oo.owner=a386`, resource names, the request host `oo-a386-pg`, and appended the local-analysis phase.

## Private PostgreSQL

`pg/up.exit` is 0. Server version `17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a386-pg`, id `42b2fa236b839fa3d95d39cc9f1ed5394947d40a8f933e215483280496a2720e`. Memory 1073741824, NanoCpus 1000000000, privileged false, network `oo-a386-eef831aa-net`, published ports `{}`. CapDrop ALL, with CHOWN, DAC_OVERRIDE, FOWNER, SETGID, and SETUID added back for the database entrypoint. Certificate SAN is `DNS:oo-a386-pg`. Secrets stayed under `/tmp/oo-a386-eef831aa-dba4-4af2-a78b-541bc9e0a386/secrets`. A scan of this evidence tree found none of those secret bytes.

## Chain

`linux/chain/docker-run.exit` is 0. `linux/chain/chain.exit` is 0. Launch elapsed about 375 seconds. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a386-eef831aa-net`, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000. Container `oo-a386-chain` was removed. Capacity recorded before the run: host cargo 0, rust builders 2 (`oo-g3-a352-m7-rust`, `oo-a389-chain`), own builder 0. Those other containers were not touched.

`linux/chain/test-live.log` records `test a327_live::live_numeric_proof ... ok`. Phase exits under `linux/chain/phases/` are 0, including `fetch`, `build-pipeline`, `build-builder`, `build-gateway`, `build-exporter`, `test-live`, `export`, `gold-complete`, `gold-partial`, `gold-formula`, `lock-hash`, `lineage-complete`, `lineage-partial`, and `analysis-proof`.

This run's complete stdout is 1527 bytes, SHA-256 `15ba2ac4a9ac5e86218741a119cb944e0098f88137c4292eb6aceeed2f5b4001`. The reviewed a344 stdout of the same length hashed to `20623e7dc575b618223ac383ceb1b2e9a9e3f9efc1695c58ea2373bcaff4b3eb`. The bytes differ. The exported CSV is 208 bytes, SHA-256 `f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d`, the same digest as that reviewed CSV, and `gold-complete.txt` says `gold_match=1`. Partial stdout is 616 bytes, SHA-256 `dde246f432828b2179176c58644688ec1cdb01efb7cb3b5449fd25f0b69c5b04`. The unit-test fixture copy was not the input of this chain.

Audit `audits/amounts.jsonl` is 1044 bytes, SHA-256 `654c199a101fc0a864c211b7df6a838f4457776c554f308c3a7dd2dcfa6f82b2`. `lineage-file` embedded those exact bytes. Derived query id is `q_49b26b99a4752eca`. Role `oo_r_clerk`. Project `a327num`. Original SQL SHA-256 `466f22e579afe5252e78ae6cffca1da0a9c4630ba8cb63440a0fc9b7a9c6957a`. Executed SHA-256 `034855baad1325ac740d6706f83a42f02549f26096fb37a14aabe0af328f0d9a`. Authorization in the manifest is `not_claimed`. Authenticated is false.

## Analysis results

Binary `/target/debug/local-analysis`, SHA-256 `8958cd849cba266be4c1b26e2041dfa1a0f3226f8b4f51dd0cf5eac853983d6a`, the binary built by the earlier locked unit test. `analysis-exits.txt`:

```
amount=0 wide=0 partial=1 cancel=1 parquet=3 xlsx=3 engine=3
```

`analysis-proof.txt` is `ok`.

Amount sum CSV is 95 bytes, SHA-256 `2edb40e84bc45e0391474c745933839d410282614c337b164ce66f53476a9a8a`, six CRLF records, no BOM, no apostrophe. Groups 1 through 5 keep `12345678901234.5678`, the leading minus on group 2, `0.0001`, `-0.0001`, and `0.0000`. Group 6 is omitted. Manifest `null_values` is 1 and `null_groups` is 0. The SVG is 1930 bytes, SHA-256 `1cda8f30081343928ec59d69dc63e68d3e3bdc175640bc0a3f1ffb6f627a5ebf`, title `金额`, desc `数值已核对。DuckDB、Arrow、Excel未准入。字体未随包提供。` Bar widths are 400, 400, 1, 1, and 0. The only `http://` URL is the SVG namespace. No `script`, `foreignObject`, or `href`.

Wide sum CSV is 133 bytes, SHA-256 `813f62ae845d4acf058403b60dcdb70e4e9af32ede60d850db045e555afbbb3f`. It keeps `1234567890123456789012345678901234.5678` and the same text with a leading minus, with no apostrophe. The chart title is `宽金额`.

The truncated export wrote an analysis and exited 1. Stderr is exactly `local-analysis: uncited`. The SVG contains `不可引用` and does not contain `数值已核对`. Manifest `citable` is false and `closes_sp09_w05` is false.

Cancel exited 1. Stderr is exactly `local-analysis: canceled`. `out/analysis-cancel` received no file. Parquet, xlsx, and DuckDB exited 3 with stderr `parquet`, `xlsx`, and `engine`. Each refusal directory still contains only `keep.txt`. No `analysis.*` name was created there. Successful directories contain no `.partial` residue.

## Unit tests and Clippy

Smoke `linux/smoke/smoke.exit` is 0 before product edits. Locked offline `cargo test --all-targets` in `linux/test` passed 7 library tests, 0 binary tests, 6 API tests, and 3 CLI tests. `test.exit` is 0. The pinned image has no Clippy component. `cargo clippy` exited 1 with `cargo-clippy is not installed` for `1.94.0-aarch64-unknown-linux-gnu`. `clippy.exit` is 1 and `docker-run.exit` is 1 for that reason. No component was installed. Clippy is not claimed.

## Failed attempts kept

- `linux/smoke/failed-attempt-1`: tmpfs `noexec` and a missing `pipefail` made the first smoke look successful while the probe did not run.
- `linux/test/failed-attempt-1`: `svg.rs` borrow error E0502. Exit 101.
- `linux/test/failed-attempt-2`: `Staged` lacked `Debug`, and `Export.turn` was unread. Exit 101.

The chain had no failed attempt.
