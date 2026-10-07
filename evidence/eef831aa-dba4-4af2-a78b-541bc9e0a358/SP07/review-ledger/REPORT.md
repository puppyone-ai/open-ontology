# REPORT

Session `eef831aa-dba4-4af2-a78b-541bc9e0a358`. Fixed base `a13b1cc80ed66c07199a41d47b31fd42530f2be9` is an ancestor of the parent commit. Product commit `f862683945a6e5a8803e3f9450d0f51cde5a98ad` on parent `d22187ea0fce8e420323660841f2da3b447d5de8`. Message: `review-ledger: persist S9/S10 decisions with audit in one transaction`.

This packet does not accept SP07-W05, SP02, or the parent goal. No acceptance ledger or count file was edited. The new component is not counted.

## What landed

`tools/review-ledger` is its own workspace. Schema `oo_review_ledger`. Contract `oo.review-ledger/v1`. CLI `review-ledger`. Library functions `install`, `enqueue`, `decide`, `batch`, `receipt`, and `history`.

A decision and its audit event commit in one PostgreSQL transaction. The compiler runs outside that transaction. A receipt row is inserted only after `description_review::apply_description_review` or `description_pack::apply_description_pack` returns an accepted document, and only when a new transaction still sees the same decision body and head candidate. Flags on every result stay `authenticated: false`, `authority: not_verified`, `management_identity: not_claimed`, `curated_applied: false`, `published: false`, `grants: not_authorized`. Receipt flags `git_applied` and `curated_files_written` stay false.

The caller is a control file plus a private credential file. Request keys that would grant, certify, publish, or name a role are refused before serde. The migration does not create customer grants or an audit-collector certificate.

## Proof that ran

Linux arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arch arm64. One private database `oo_a358_review` on bridge `oo-a358-review-net`, alias `pg.a358.internal`, no host port. Builder 6 GiB, 1 CPU, `--cap-drop ALL`. PostgreSQL 1 GiB, 1 CPU, `--cap-drop ALL` plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, `SETGID`. `RUSTUP_TOOLCHAIN=1.94.0`. `--pull never`. No host network, docker socket, host home, or shared cargo cache.

Smoke, before product source: `smoke/docker-run.exit` is `0`. `smoke/console.log` starts `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `smoke/hostconfig.txt`: `NetworkMode=none`, `OOMKilled=false`, `Memory=6442450944`, `CapDrop=["ALL"]`.

Write phase, `proof/write.log`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`:

- `sha256::tests::known_answers` ok
- `tests/contract.rs`: 7 passed
- `private_postgres_matrix` ok in 1.42s
- container exit 1 after that, because `cargo-clippy` is not installed for `1.94.0-aarch64-unknown-linux-gnu`

`proof/write-inspect.txt`: `exit=1 oom=false privileged=false network=oo-a358-review-net memory=6442450944 nano=1000000000`. `proof/write-caps.json` is `["ALL"]`.

The live matrix, in that one test, covered install replay, enqueue revision conflict, same-candidate hash conflict, auditor enqueue refusal, scope refusal, candidate.changed, stale evidence, reviewer_not_owner, owner record with `grants=not_authorized`, classification.missing, `pii.unknown` with zero decision rows, description-pack command `{` refused by the ledger as `compiler.refused` with zero receipts, a description-review receipt `bound` then `replay` with an empty curated directory, batch `partial` and `complete` with `applied=false`, `batch.accept_all` writing nothing, audit.rollback of a poisoned idempotency key with the decision row absent, a later candidate change refusing a new receipt, CLI `not_grant` writing nothing, and history. The marker `proof/history.sha256` is `da28bce2e8b4a697cd0589fa2ac2e47a9c1055e911ac32394ce4034ffd4833e6`.

Clippy, separate command, `proof/clippy.log`: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `rustup component add clippy --toolchain 1.94.0`, `cargo clippy --locked --all-targets -- -D warnings`, `Finished dev profile in 0.67s`. `proof/clippy-inspect.txt`: `exit=0 oom=false memory=6442450944 nano=1000000000`. `proof/clippy-caps.json` is `["ALL"]`.

Restart, `proof/restart.log`: `docker restart` of `oo-a358-review-pg`, then `cargo test --locked --offline --test live`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `private_postgres_matrix` ok in 0.12s. That phase only re-reads history and compares it to the marker. `proof/restart-inspect.txt`: `exit=0 oom=false memory=6442450944 nano=1000000000`.

`commit.unknown` is implemented as a lost commit reply and was not fault-injected. A successful description-pack receipt (`packed` or `reviewed_unchanged`) was not executed against PostgreSQL. The live pack proof is the refusal of `{`.

## Resources

CA certificate SHA-256, `proof/ca.sha256`: `9b0a25ad6e753648cc741428c470381250be150c5de22c394de4dde5c8fe0d63`. Keys and passwords stayed in `/tmp/oo-a358-review-secrets` and are not in this tree. Owned cleanup is `CLEANUP.md`.
