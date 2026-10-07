# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a313`. Read `execution/decisions/gateway-http-dependency-baseline.md`, then `REPORT.md` and `FINDINGS.md`.

The root workspace now names Axum 0.8.9, Tower 0.5.3, and tower-http 0.6.11 with default features off. `Cargo.lock` is still the fixed-archive bytes `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. The loopback proof is `linux/proof/` (`docker-run.exit` 0, 4 tests passed, clippy 0). Failed proof attempts 1–4 and smoke attempt 1 stay beside it.

Foundation commit: `773c461104729d6c289eae8640c7c2a28207c447`.

Status of this baseline: **UNAPPROVED**. The four review tasks below passed and still do not approve a consumer manifest. This handoff does not accept a parent goal, a release, or a public Gateway API. `oo-gateway` does not link these pins. PostgreSQL cancellation on timeout is not implemented. No JWT or OIDC crate was added.

## Reviews

- `linux/review-1-pins/REPORT.md`. Live crates.io checksums match the committed metadata. Axum 0.8.9 is still the newest Axum.
- `linux/review-2-lockdiff/REPORT.md`. Archived lockdiff: `added=5`, `changed=0`, `denied=0`.
- `linux/review-3-rerun/REPORT.md`. `git archive` of the foundation commit, fresh target volume, 4 tests passed, clippy exit 0. Container `83cb5ecac838c956bc82054f1219002d8804e3720eb7e693707e9b4eb6d51897`.
- `linux/review-4-scope/REPORT.md`. The foundation commit stays inside `Cargo.toml`, the decision file, and this evidence tree. `Cargo.lock` blob `2990f901ea98a7b465f462cce8742bc5c51c13aa`. a207 commit `672129aa` is still an ancestor.

## What the next consumer card has to do

Wait for an approved fixed archive of this pin. Then, on the one crate that will serve HTTP, set `axum`, `tower`, and `tower-http` to `workspace = true` and run a targeted fetch. The root lock diff for that card must show only these added triples, with `changed=0` for every package already in the lock:

- axum 0.8.9 `31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90`
- axum-core 0.5.6 `08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1`
- httpdate 1.0.3 `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9`
- matchit 0.8.4 `47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3`
- mime 0.3.17 `6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a`

The checksum oracle is `probe/Cargo.lock`. Use `GlobalConcurrencyLimitLayer` when one permit must cover every route. Use `TimeoutLayer::with_status_code` for the HTTP 408. Leave query, form, json, and tower-http `auth` off unless a later contract adds them. Do not blanket-update the workspace. Do not rewrite child locks that still point at the old baseline. Connector manifests stay a207-owned. a207 commit `672129aa` is still in history.

## Builder and cleanup

Green proof container `434367fb3bfdcd3b83f8cc1732acd6874ca96a0939d172d1ba050cb9ab803bc5` and review container `83cb5ecac838c956bc82054f1219002d8804e3720eb7e693707e9b4eb6d51897` were removed. `CLEANUP.txt` records removal of volumes `oo-a313-eef831aa-cargo`, `oo-a313-eef831aa-target`, and `oo-a313-eef831aa-review-target`, network `oo-a313-eef831aa-net`, and `/tmp/oo-a313-eef831aa-dba4-4af2-a78b-541bc9e0a313`. No prune. Other owners' containers stay up.

`linux/proof/SHA256SUMS` does not include the green run's hostconfig tail. See FINDINGS F7. The review files were written after that inventory.
