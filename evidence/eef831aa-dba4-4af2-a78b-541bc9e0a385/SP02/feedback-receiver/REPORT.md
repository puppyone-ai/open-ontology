# SP02-W06 / SP13-W04 feedback receiver queue slice

Session `eef831aa-dba4-4af2-a78b-541bc9e0a385`. This records the owned-path proof for `tools/feedback-receiver`. It is not full SP02-W06, not full SP13-W04, and not parent or goal acceptance.

SP02-W06 in the plan is the collector and the feedback receiver. The collector and gateway WAL seam is the earlier `eef831aa-dba4-4af2-a78b-541bc9e0a371` proof. This card is the feedback receiver only. SP13-W04 asks for gap, reject, and failure feedback to become review candidates, with size limits, dedup, and a boundary between generated text and grants. This card is that queue slice: bounded untrusted items become stored candidates and sources that `GET /v1/reviews` can return. It does not add a suggestion job, a review page, or the G-gate that runs after a person accepts a candidate.

## What was built

`feedback-receiver` is a rustls mTLS HTTPS process. Workload identity is the client-certificate URI from `audit_ingest::certificate_uri`. The registry maps that URI to a project. `X-Project` is not identity. `tools/audit-ingest` was not modified.

Candidates come from `selfbuild_suggestions::produce`. Seals are `canonical_sha256` of the operator catalog, relationships, profile, and the bounded items passed into `produce`. A caller field is not authentication. `published`, `grants_issued`, `roles_changed`, `knowledge_verified`, and the other closed effects stay false. No Gateway config is passed, so the producer does not mark knowledge verified.

`POST /v1/feedback` stores a receipt, the untrusted feedback rows, and the candidate rows with sources. The same receipt and the same body replay the stored ack, including after process restart. The same receipt with a different body is HTTP 409 and commits nothing new. A cross-project item is HTTP 403 and commits nothing. A body project that is not the certificate project is HTTP 403. A `Content-Length` above 32768 is HTTP 400 and the body is not read into the queue. The injection string stays in `feedback_item.item_text` and in `source.feedback_text` on channel `untrusted_feedback`. SQL templates and other channels do not contain it. `GET /v1/reviews` returns the certificate project's stored candidates. The ack is not the only copy.

The PostgreSQL role `feedback_receiver` has `INSERT` and `SELECT` on `oo_feedback_receiver.receipt`, `feedback_item`, and `candidate`. It has no `UPDATE`, `DELETE`, or `TRUNCATE`. It cannot `CONNECT` to `oo_audit`. `audit_ingest` cannot `CONNECT` to `oo_feedback`. There is no `oo_r_reviewer` database role. That name is only the suggestions scope string. A receipt whose `effects.published` is true is rejected by `receipt_not_published` (SQLSTATE 23514).

## Proof

One private PostgreSQL 17.10 and one Linux arm64 Rust 1.94 builder. Namespace network `oo-a385-net`. No host network, no published ports, no docker socket, no host home, no shared cargo cache. Host `cargo` and `rustc` were not used.

Images, already present, not pulled:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

Recipe files, checked at launch and left unmodified:

- `deploy/trusted-demo/pg/up.sh` `904aed7ed10156576252da4be7ad828832aefe4628d15f67c1436f475cad9bff`
- `deploy/trusted-demo/pg/pg-wrap.sh` `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`

The comment inside `pg-wrap.sh` cites an older hash. The proof uses the file-on-disk hash above. Origin commit of those two scripts: `10b78993641c88154c75c22219c79c1566e2cecd`. This card does not build `pg_query` and does not use libclang.

Pre-edit smoke, before source edits: `preedit-smoke.txt`. `rustc 1.94.0 (4a4ef493e 2026-03-02)` on `aarch64-unknown-linux-gnu`, exit 0. `std::fs::File::try_lock` ok.

Launch recorded `raw/head.txt` `c5913f893e36b5745f400af83e11894d7725b9c2`. The 24 immutable inputs (root manifests, `rust-toolchain.toml`, the two pg scripts, and every file under `tools/audit-ingest` and `tools/selfbuild-suggestions`) matched before and after the run. Rechecked against the working tree after later commits: `diff=0`. `rust-toolchain.toml` still says 1.98.1. The proof sets `RUSTUP_TOOLCHAIN=1.94.0`.

Final builder: `rustc 1.94.0`, `cargo 1.94.0`, `clippy 0.1.94 (4a4ef493e3 2026-03-02)`. PostgreSQL `pg/server_version.txt` is `17.10 (Debian 17.10-1.pgdg12+1)`.

Passing run phase exits, all `0`: `lock`, `lock-subset-fast`, `fetch`, `test`, `clippy`, `secret-scan`. `lock-mode.txt` is `seeded`. The child lock was not regenerated. `inside.exit` is `0`. `builder.exit` is `0`.

`cargo test --locked -- --test-threads=1`:

- `live_feedback_queue` passed in 0.58s
- `child_lock_is_a_root_checksum_subset` passed
- `injection_stays_untrusted_and_matches_the_candidate_oracle` passed
- `cross_project_duplicate_key_and_bounds_are_refused` passed

Library, binary, and doc tests ran and contained no tests. Clippy `--locked --all-targets -- -D warnings` finished in 0.48s.

`pg/receipt-counts.txt` after the run: `shop=1`, `other=1`. `pg/privileges.txt` shows `feedback_receiver` and `audit_ingest` only, both without superuser, createdb, createrole, bypassrls, or inherit. Table grants are `INSERT` and `SELECT` only. `feedback_on_audit=f`, `audit_on_feedback=f`, `can_update=f`, `can_delete=f`, `can_truncate=f`.

`raw/live-result.txt` is append-only across launches. The first thirteen lines are the attempt-3 prefix, which stopped at `stored=ok`. The passing run continues:

```
accept=ok
dedup=ok
restart=ok
conflict=409
cross_project=403
body_project=403
oversized=400
bound=400
stranger=403
admin=405
other_project=ok
review=ok
stored=ok
privileges=ok
audit_isolated=ok
live=ok
```

`raw/immutable-before.txt` and `raw/immutable-after.txt` match. Secret scan exit 0. No private-key markers are in the owned tree.

## Failed attempts

Phase logs under `linux/proof/phases/` are the passing run. Earlier failures are under `raw/failed-attempt-1`, `raw/failed-attempt-2`, and `raw/failed-attempt-3`. `raw/console.log` keeps all four launches because `inside.sh` appends.

1. `CARGO_BIN_EXE_feedback_receiver` was unset. Cargo sets `CARGO_BIN_EXE_feedback-receiver`. The library compiled. The live test did not run. `test.exit` 101, `clippy.exit` 101.
2. `tests/rules.rs` moved `expected` into the prior slice and then borrowed it. The library, binary, and `live.rs` compiled. The live test did not run.
3. The live test reached `stored=ok`, then the published-true insert used `$3::jsonb` with a text parameter. `sqlstate` was `none`, not `23514`, because the client rejected the type before the check ran. Clippy failed on `tests/rules.rs`: `cloned_ref_to_slice_refs`, `bool_assert_comparison`, `collapsible_if`. The kept volume `oo-a385-pgdata` held the shop and other receipts. That volume was removed before the passing launch. `oo-a385-cargo` and `oo-a385-target` were kept for that launch and removed after it passed.

## Cleanup

`CLEANUP.md`: the passing run removed `oo-a385-proof`, `oo-a385-pg`, volumes `oo-a385-cargo`, `oo-a385-target`, `oo-a385-pgdata`, network `oo-a385-net`, and the private secret directory. No prune. Foreign containers were not removed.
