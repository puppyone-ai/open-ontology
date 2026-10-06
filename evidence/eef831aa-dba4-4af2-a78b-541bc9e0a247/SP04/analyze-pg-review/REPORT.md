# a247 review of analyze-pg

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a247`. Fixed commit `6ac2216505723b51ae116b1938aca47846ade35f`. Parent `4f6f44cb9a3ab8e3651ea7d5e96800c41d7b7d49`. Message `pipeline: add the analyze-pg operator command`.

Verdict: **APPROVE_LIMITED**.

The deciding proof compiled `init-pg`, `oo-pipeline`, and `analyze-pg` from that commit. On a private TLS PostgreSQL 17.10 it initialized a fresh target, imported two source rows while `reltuples` was `-1`, refused a wrong CA, a runtime caller, a plain role, a stale generation, and an absent mapping, then ran `analyze-pg`. The receipt kept `published` false and `estimate_is_exact` false. `estimated_rows` was 2, the physical `count(*)` was 2, the sentinel stayed `-1`, and the source audit did not move. A fixture column-ACL install then loaded the policy catalog with `UnknownRows::Refuse` and a typed `SELECT count(*) FROM app.party` returned 2. Catalog `estimated_rows` matched the receipt.

This review does not accept M1-D06B, the parent goal, or a release. No product, library, manifest, or lock file was edited. There is no `AGENTS.md` at the repository root. The plan ledger was not updated.

## Owned product at the fixed commit

The commit adds only these three paths.

| Path | Git blob | SHA-256 |
|---|---|---|
| `contracts/pipeline/analyze-pg-cli-v1.md` | `17087bc34b759045088019304975638899f75431` | `2364dd4f6ee66fa6f0565ad5cc624bae79460f7b46024bb7900d387c063c35c5` |
| `crates/oo-pipeline/src/bin/analyze-pg.rs` | `fd84d315d6dc0fee368c9323304fa1ebcf285700` | `ebdb96007ab8534dedee09f39ac5744f77dd58e8b4f6c78fe714c1729e9156f4` |
| `crates/oo-pipeline/tests/analyze_pg_cli.rs` | `23bac315040c21330d4e903c82cd2b26fd466eff` | `047f0225019da3852b0bbc8e6ad3ff1820f4bd34ea4c7f28caec4bffb81f41c4` |

`linux/smoke/archive-blobs.txt` matches those git blobs to the extracted archive. SHA-256 values are `linux/smoke/owned-sha256.txt`, matched again by `linux/overlay/phases/frozen-match.exit` 0 and `linux/overlay/phases/product-hash.exit` 0. `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` (`linux/overlay/lock-hash.txt`).

The review test is not in the commit. `git hash-object` is `ac6bede001ce697ea0f9c4ec70e6a8695adfa9a2`. Its proof SHA-256 is `587555796d3e3dd8c9ccf4cc46accf1d6feafd8693ae6e7fb8520caf9601bf41`.

## Archive and recipes

Partial archive of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts` from `6ac2216505723b51ae116b1938aca47846ade35f`. Listed files 962, extracted files 962. `rust-toolchain.toml` was moved aside on the private copy. `RUSTUP_TOOLCHAIN=1.94.0`.

Recipe origins and the a247 deltas are in `origin/recipes.txt`. Launch and report shape follows a241, a204, a200, and a199. The archive is the fixed commit itself. Smoke confirms `overlay/analyze_pg_a247_review.rs` is absent (`linux/smoke/overlay-absent.txt`). The overlay copies only that file onto the private archive.

## Linux smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, arm64, `--pull never`. Container `oo-a247-smoke` id `030ebf6b9eabed0f83ef1930ea10b6a8424a57d4599fe30bcbd2a6bdbe2e15a4`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL, privileged false, port bindings `{}`, network `oo-a247-eef831aa-net`, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=1g`. The launch script removed the container. `docker-run.exit` 0. `smoke.exit` 0. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `clippy.exit` is `deferred_until_overlay`.

`cargo test --locked --offline -p oo-pipeline --test analyze_pg_cli -- --test-threads=1` wrote `test result: ok. 4 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.02s`. The ignored test is `analyze_pg_binary_on_private_postgres`. `OO_A247_LIVE` and `OO_ANALYZE_PG_HOST` were unset. No live oracle was written by smoke. An empty cargo volume recorded `phases/fetch-offline.exit` 101, then the locked online fetch recorded `phases/fetch.exit` 0. `cargo update` was not invoked. Capacity before launch: `host_cargo=0 rust_builders=2 global=2 cap=10`.

## Private PostgreSQL

Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arm64, PostgreSQL 17.10. Container `oo-a247-pg` id `cfac53998c2e665651eba7c60e0d963317cfe51e757618c3873782ae0f9e5e70`, alias `oo-a247-pg`, SAN `DNS:oo-a247-pg`, admin `oo_a247_admin`. `up.exit` 0. Privileged false. Host port bindings `{}`. `autovacuum=off` via `pg/pg-wrap.sh`. The builder mount listed only `admin.pass`, `ca.crt`, and `wrong-ca.crt`.

## Overlay proof

Container `oo-a247-proof` id `00d501fa59875a137a792a2a7b6b781b1a5bdf81e40b7223ed4b109ce5a35b16`. Same Rust limits. `OO_A247_LIVE=1`. `docker-run.exit` 0. `overlay.exit` 0. Capacity before this launch: `host_cargo=0 rust_builders=3 global=3 cap=10`.

Compiled binaries, SHA-256 from `linux/overlay/bins-sha256.txt`:

```text
ac270b3ba9b8a89ac681e0a776f67f188614fb374c6541c2cfb7c04e4f9bfeed  /target/debug/init-pg
d05519e83c35ddf921293dea2b253a6758565862c8c3e3b79e14068c40895f6a  /target/debug/oo-pipeline
7e49b035043701fc033c474c1e3581fcf8c54d79594a9816dcf2ad4170e73916  /target/debug/analyze-pg
```

`cargo test --locked --offline -p oo-gateway --test analyze_pg_a247_review -- --test-threads=1 --nocapture` wrote `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.04s`.

`cargo clippy --locked --offline -p oo-pipeline --bin analyze-pg --test analyze_pg_cli -- -D warnings` exited 0. The log shows two unfulfilled `dead_code` expectations in `crates/oo-connectors/vendor/sqlx-postgres`, then `Finished`. `cargo clippy --locked --offline -p oo-gateway --test analyze_pg_a247_review -- -D warnings` exited 0 with no warning. `clippy.exit` 0. `phases/unsafe.exit` 0 (`no unsafe block`). `phases/secret-scan.exit` 0.

Live oracle `live/oracle.txt`:

```text
audit_before=2
import_state=awaiting_publish
import_rows=2
reltuples_before=-1
sentinel_before=-1
wrong_ca=4
wrong_ca_fact=target connect
runtime=5
runtime_fact=runtime caller
plain=5
plain_fact=target role
stale=6
stale_fact=stale generation
absent=6
absent_fact=mapping absent
reltuples_after_refusal=-1
published=false
estimate_is_exact=false
reltuples_after_bits=1073741824
estimated_rows=2
count_rows=2
disposition=reusable
second_published=false
second_exact=false
second_bits=same
sentinel_after=-1
admin_idle_in_transaction=0
maintenance=explicit
open_transaction_input=absent
timeout_campaign=not_run
forged_relation_campaign=not_run
source_audit_delta=0
source_reader_writes=0
source_reltuples=-1
acl_install=fixture
policy_catalog=loaded
unknown_rows=refuse
catalog_estimated_rows=2
query_status=succeeded
query_count=2
statistics_published=false
```

`live/analyze.stdout` is one result document, protocol `oo.pipeline.analyze-pg-result/v1`, project `a247src`, generation 11, digest `29d4555109fe88b7bba99f2ee85b09f6c224ba5b82c93351c5fa25ada169283e`, physical name `pt_a1548df9aa6b4bc3e875f6426388be8e0cda505af8f6edf0fac6092b842f`. The document has no `transaction` field. It records `lock_timeout_ms` 5000 and `statement_timeout_ms` 5000. This review did not run a lock, statement-timeout, or forged-relation campaign.

## Failed attempts

Attempt 1 (`linux/overlay/attempt-1/`) completed the live chain, the oracle checks, and `analyze-pg` Clippy. Review-test Clippy exited 101 because `login_file` was never read. `phases/clippy-review.exit` 101. Its `live/` copy is that attempt's oracle.

Attempt 2 (`linux/overlay/attempt-2/`) reused the same cluster. `init-pg` exited 6. Bootstrap roles `oo_target_owner` and `oo_pipeline_runtime` are cluster-wide and were still present, so `CREATE ROLE` on the fresh databases was refused. `phases/test.exit` 101.

Attempt 3 (`linux/overlay/attempt-3/`) dropped those roles, passed init, import, the refusals, and `analyze-pg`, then `install_column_acl` returned `GrantMismatch`. Cluster role `oo_r_clerk` remained from attempt 1 and had no grants on the new physical table. `phases/test.exit` 101.

The harness then drops `oo_pipeline_runtime`, `oo_target_owner`, and `oo_r_clerk` before init, and reads the login password from `login_file`. Product sources were not edited. The green console is `linux/overlay/console.log`.

## Cleanup

`cleanup/run.sh` removed `oo-a247-pg`, volumes `oo-a247-eef831aa-cargo`, `oo-a247-eef831aa-target`, and `oo-a247-eef831aa-pgdata`, network `oo-a247-eef831aa-net`, and `/tmp/oo-a247-eef831aa-dba4-4af2-a78b-541bc9e0a247`. Smoke and proof containers were already gone. No prune. Foreign containers were left in place. Transcript: `CLEANUP.txt`. `SHA256SUMS` lists 257 files. The manifest file is the writer output and is not an entry in itself.
