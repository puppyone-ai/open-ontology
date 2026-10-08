# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a387`. Author record for `tools/session-control-api`. This file does not accept the card, SP11, or the parent goal. Codex updates the ledger. No independent reviewer has signed this tree.

Read `REPORT.md` and `FINDINGS.md` first. Raw exits are `linux/impl/*.exit`, `linux/impl/attempt1` through `attempt5`, `linux/precode/*.exit`, and `pg/up.exit`. `SHA256SUMS` lists this evidence tree and the crate. `SOURCE-INVENTORY.txt` lists the crate files. `SHA256SUMS` does not include itself.

## Demonstrated close

Localprocess S1–S11 close ran against one private PostgreSQL 17.10. `linux/impl/test.exit` is 0. `localprocess_s1_through_s11_closes_and_restarts` passed in 28.95s. The accepted HTTP body has phase `closed`, eleven steps, S8 confirmed absence, S5 skipped, every other step confirmed, `production_signed` false, and `reducer_physical_release_certified` false. The test also observed token removal, pool replacement, backend `application_name` gone, and harness pid absent from `/proc`.

A fixture-only receipt stays `releasing` and is not acceptable. HTTP returns 409 for it. The restart proof is `fixture_stage` in the test process, then HTTP finish. A snapshot file blocks finish with 409 `snapshot_present_uncollected`. A crash inside `session_release::run` before S1–S4 and S8 are confirmed stays `unproven_resume` and is not closed.

Profile `production` does not listen. `production_signed` stays false. Missing `runsc` is `runtime_unavailable`. Present `runsc` is `production_driver_absent`. This handoff does not sign production.

## Re-run

Images, unchanged:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

One Rust builder, linux/arm64, 6 GiB, 1 CPU, CapDrop ALL. One private database, 1 GiB, 1 CPU, no host port. Keep the committed `tools/session-control-api/Cargo.lock`. Do not run `cargo update` or edit the root manifest or root lock. The one offline `cargo generate-lockfile` already happened; see FINDINGS.

```text
cargo test --locked --offline --all-targets -- --test-threads=1
cargo clippy --locked --offline --all-targets --keep-going -- -D warnings
```

Set `OO_ROOT_LOCK` to the root lock and `OO_A387_FIXTURE` to a fixture directory with `endpoint.txt`, `ca.crt`, and password files mode 0600. Passwords travel in stdin JSON. They are not argv and they are not in this evidence tree.

## Boundaries

Write boundary of this card: `tools/session-control-api/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a387/**`. Session-store, session-transition, session-release, and gateway-token-store were read and called. They were not edited. The frontend and the main ledger were not edited.

Owned cleanup is in `cleanup.txt`. No a387 container, volume, or network remained at the observation after the green run. Foreign databases were not removed. `docker prune` was not run. Secrets under `/tmp/oo-a387-eef831aa-dba4-4af2-a78b-541bc9e0a387` were removed with that tree. A later scratch dir `/tmp/oo-a387-draft` was removed while these reports were written.

The owned commit subject is `session-control-api: close an own session through S11 in localprocess`. Parent at report time: `3b35c82795a0ac9a4d9c0838d93e99703a938266`.
