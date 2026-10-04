# a152 target installer review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a152`. Verdict: **APPROVE LIMITED** for the schema installer `oo_pipeline::target_installer` at `4ed22b8c1189e72b06b20a45335860ac9a3ea44e`, tree `fafb0fe6752b8240eb2b29025fc38bccdfb7144d`, parent `0791e971f49cd8afc646d80179241ddbcfaa1a1c`. Author evidence is `e9f1b773`. This review did not edit production, manifests, the lock, specs, or the ledger.

The approval is the production entry points `install` and `verify_readiness` on one caller-owned `sqlx::PgConnection`. It is not SP04-W05, not SP04-W05-a, not AT-PL-05, not the Loader, not a publication, not fenced `COPY`, and not source-reader admission.

## What was approved

On one private PostgreSQL 17.10, the independent oracle called `install` and `verify_readiness`. It did not use `psql`, `tests/pg_fixture.sh`, or `segment_preflight_correction::apply_reference` as the proof.

| Route | Result |
|---|---|
| Advisory lock already held | `55P03` after the 15s lock timeout. `_oo` absent. Readiness `not_installed`. |
| Empty database, no product roles | `Bootstrapped`. Replay left `applied_at` unchanged. No v2 row. Publications 0. Receipts 0. |
| `oo_pipeline_runtime` member | `verify_readiness` returned the compiled digests. `install` returned `NotAdministrator`. |
| Valid v1, no v2 row | `UpgradedFromV1`. No v2 row inserted. Prior `applied_at` unchanged. |
| Valid v2 plus one legal receipt | `UpgradedFromV2`. v1, v2, and the receipt row stayed. |
| Valid v3, mapping row and `physical_mappings` absent | `InstalledMapping`. Only the mapping marker was added. Bootstrap, v1, and v3 `applied_at` stayed. Replay left them unchanged. |
| v1 with the segment constraint dropped | `OO002`. Rows unchanged. Readiness `incomplete`. |
| Stored v3 checksum is a lie | `OO001`. The lie stayed. No mapping row. Readiness `checksum_mismatch`. |
| v1 receipt that breaks the closed segment rule | `OO021`. No v3 row. Receipt unchanged. Readiness `incomplete`. |

Embedded SHA-256 values matched the file bytes and the digests returned by `verify_readiness`. `0002` is not compiled into the installer and was not executed. `HISTORICAL_V2_SHA256` matches the frozen `0002` bytes.

`0000` through `0003` have an empty diff since the a108 approval `6cd955af38ce426bb1102a6e078e8580febed776`. `0004_physical_mapping.sql` and `src/physical_mapping.rs` have an empty diff since the a142 approval `b5415544c9eb8d608eafd411884627dda9ee7c84`. Those reviews were not rerun. This review did not open a privilege, security, or 180-cell matrix.

## Product limitation

Roles are cluster-wide. `fresh_catalog` in `target_installer.rs` is false when `oo_target_owner` or `oo_pipeline_runtime` already exists. A second empty database on the same cluster is therefore not the fresh row. `0000` is not rewritten to skip `CREATE ROLE`.

The oracle created that second database after a successful bootstrap and called `install` without dropping the roles. The call returned `OO002`, message `target-fence-receipts/1 segment relation is absent`. The transaction rolled back. The second database had no `_oo` schema and `verify_readiness` returned `NotInstalled`. The first database's migration rows were unchanged.

This installer bootstraps one fresh database on a cluster that does not already have the product roles. It does not install an arbitrary additional target database on a cluster where those roles exist. That is not enterprise any-target readiness. The SQL was not changed.

## Images and smoke

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, cargo `1.94.0 (85eff7c80 2026-01-15)`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Both were local. Pull was `never`.

Smoke, archive of `4ed22b8c` with no overlay: `archive_hashes_match=1`. `cargo test -p oo-pipeline --lib` was `ok. 9 passed; 0 failed`. `cargo clippy -p oo-pipeline --all-targets -- -D warnings` exited 0. `sqlx-postgres` printed two unfulfilled `dead_code` expectations. Those lines are in the dependency. The crate source was not edited. Container `oo-a152-smoke` `f423def6076e66e1a991feb948addcd6bedd35eb71a344cc61d89451f8b32bb6`, cap-drop ALL, privileged false, private network, no host port, elapsed 37 seconds. `docker-run.exit=0`.

Probe attempt 1, `linux/probe/attempt-1/`, exited 2 in the harness. The fresh image has no Clippy component. The smoke container had installed Clippy into its own writable layer. The oracle did not start. Production files were not changed.

Probe, same archive plus `overlay/target_installer_a152_review.rs` (`de66c0584b1537e2427097be6c1a0f1e2a1a1938bbbcca6e61747159e219f787`): `cargo test -p oo-pipeline --test target_installer_a152_review -- --include-ignored` was `ok. 2 passed; 0 failed` in 15.75 seconds. `production_files_unchanged_after_probe=1`. Container `oo-a152-probe` `ba4148c94ab6b53bed17df77bfe4798f4417af39c759d5eff273b75d8fc8de41`, same isolation, elapsed 19 seconds. `docker-run.exit=0`. The password stayed in a file under `/tmp` and is not in this evidence tree.

## Boundary

`_oo.schema_ready`, `_oo.segment_contract_ready`, and `_oo.segment_preflight_ready` still compare a caller-supplied digest. The production check is `verify_readiness`, which returns `VerifiedReadiness` only from the compiled bytes. One arm64 host and PostgreSQL 17.10 were exercised. No second architecture, no other server major, no customer gate, and no release.
