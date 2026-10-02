# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a115` analyzed frozen `99b326b18add833896128ca344d10452b0c34c30` (tree `f397a2acada92905449102873e70908290e2383c`, parent `e0726071950c54383b2fe6c1831e419045b1ebe6`). It did not take a builder slot and did not take the root writer. a112 and a114 stay the builder slots. R1 and trust review stay independent.

## Decision to activate

Coordinator chooses one:

1. Schedule a scoped implementation of the `[graph].targets` edit in `REPORT.md`, on a builder slot after one of a112 or a114 releases, with an exclusive root writer for `ci/deny.toml` only.
2. Refuse target scoping. Then there is no repair in the current pins. Leave `cargo deny --config ci/deny.toml --locked check` failed, and do not widen the allow list to paint it green.

This session does not approve either choice.

## If (1) is scheduled

Root writer may change `ci/deny.toml` and nothing else.

Intended edit, inside the existing `[graph]` table:

```toml
targets = [
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
]
```

Leave `all-features = false` and `no-default-features = false`. Use bare strings, not `{ triple = "...", features = [...] }`. Do not add `CC0-1.0` or `CDLA-Permissive-2.0`. Do not add `exceptions`, `ignore`, `clarify`, `exclude`, or `exclude-dev`. Do not pass a CLI `--target` that overrides this list. Do not edit `Cargo.toml`, `Cargo.lock`, `crates/oo-connectors`, `crates/oo-sdk`, `crates/oo-cli`, vendor, `supply-chain/policy.md`, specs, plans, or any other evidence directory.

The lock must still contain `tiny-keccak` 2.0.2 and `webpki-root-certs` 1.0.9 after the edit. Those rows are the wasm32 record.

## Validation the implementer runs

Linux Rust 1.94, pinned `cargo-deny` 0.20.2, same image family as a110 (`rustc 1.94.0`). First smoke is arm64. The deny process itself evaluates both configured triples in one run; that is not the two-architecture product matrix.

Record raw exits. Do not announce success from this handoff.

- `cargo deny --config ci/deny.toml --locked check` after the edit. Keep stderr. The pre-edit failure stays in a110 `deny.err` (sha256 `6368b7928790e9465feac64746391b951f9d2cdccb04b7347ccaaa3f6ac87d25`). Do not overwrite that file.
- `cargo tree -p oo-connectors --locked --offline --target aarch64-unknown-linux-gnu -i tiny-keccak@2.0.2`
- the same tree at `--target x86_64-unknown-linux-gnu`
- `cargo tree -p oo-sdk --locked --offline --target aarch64-unknown-linux-gnu -i webpki-root-certs@1.0.9`
- the same tree at `--target x86_64-unknown-linux-gnu`
- `cargo tree --locked --offline --target all -i tiny-keccak@2.0.2` and `-i webpki-root-certs@1.0.9`

Linux inverses should print nothing. The all-target inverses should still show `compile-time-rng` and the wasm32 `webpki-root-certs` edge. If a Linux inverse still shows either crate, stop. The predicate argument was wrong, and the allow list is still not the fix.

Product regressions, only because the prepared lane asked for them if the graph moved. The compiled Linux graph is not expected to move:

- PostgreSQL live Arrow gold already used by a110: `record_batch`, decimal coefficient, canonical hash, caller CA, wrong host. Needs a real fixture and a PG slot.
- `cargo test -p oo-connectors` without `OO_PG_FIXTURE` is the known exit 101 at `crates/oo-connectors/tests/live_pg.rs` (`live_source_catalog`). Report it as a missing fixture. Do not relabel it as a pass or as a new failure caused by this edit.
- SDK, if the slot includes it: `RootTrust::Platform`, `OnlyPem`, wrong CA, wrong hostname, `no_proxy`, retry. A manifest diff for `oo-sdk` should be empty. If it is not empty, stop and treat the TLS graph as changed.

Do not run an all-workspace matrix. amd64 product tests stay at the integration node unless the x86_64 license tree contains a crate the arm64 tree does not.

No new pin is in this handoff, so there is no new 168-hour, yank, or license-body package to admit. If the implementer adds one anyway, that package needs source, checksum, license bodies, NOTICE, OR choice, AND obligations, `version.created_at` at least 604800 seconds before adoption, `yanked = false`, and MSRV ≤ 1.94, then an independent review. This document does not pre-approve that package.

## If (2) is chosen

Do not edit `ci/deny.toml`. Hand the next session F9 from `FINDINGS.md`: Arrow 60 has no published successor as of 2026-10-03, and `rustls-platform-verifier` 0.7.1 still depends on `webpki-root-certs` for wasm32. The unsigned gate is the current allow list plus an empty `targets` list.

## Commit boundary

This handoff commits only `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a115/SP00/dependency-license-analysis/`. It does not commit product files, and it is not a parent-commit approval or a release approval.
