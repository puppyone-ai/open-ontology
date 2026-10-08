# SP07 B04 self-build control API

Session `eef831aa-dba4-4af2-a78b-541bc9e0a373`. Linux arm64, Rust `1.94.0 (4a4ef493e 2026-03-02)`, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Private Postgres `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` container `oo-a373-pg` stayed up. Databases `oo_selfbuild`, `oo_identity`, and `oo_review` were kept.

## Proven this run

`tests/identity_joint.rs` exit 0 in 0.82s after a 4.07s incremental compile.

- Caller JSON is not authority.
- A real TLS `getIdentityCurrent` envelope for `fixture-reviewer` on project `beta` with `review.decide`.
- A wrong `X-OO-CSRF` returns 403 `csrf.rejected` with `management_identity` `not_verified`.
- `decideReviewItem` reject persists with `ledger_authenticated` false, `management_identity` verified, `published` false, `grants` `not_authorized`.
- The same body replays. A different choice with the same idempotency key returns 409 `idempotency.conflict`.
- A missing cookie returns 401.

`tests/contract.rs` `missing_cookie_is_401_and_closed_identity_is_503` exit 0. Ten other contract tests were left unrun. `cargo clippy --locked --offline --all-targets -- -D warnings` exit 0 in 1.29s. `rustc` stayed 1.94.0 after clippy.

The identity hold against archive `a6f92da8a0bf872f8a30efb116c2c7b14463505b` exit 0. Install replayed on the existing `oo_identity` database. Bootstrap issue returned `bootstrap.unavailable`. A fresh reviewer login still reached `install_state=initialized`. The hold does not drop the schema. Ledger fetch and ledger build were skipped because their previous exits were 0 and `/target/ledger/debug/review-ledger` was still present.

## Open

Original E56, SP07, the parent goal, customer IdP, publication, the harness, and the full role matrix remain open. The 100k S0–S10 workflow was not run. `compiled_pack` was not produced. Identity product `a6f92da8` is unaccepted. Contract and schema bytes remain `089b00d7f70da1bc42f7e201dfa070748c651d5b`.

Raw exits are under `linux/raw/`.
