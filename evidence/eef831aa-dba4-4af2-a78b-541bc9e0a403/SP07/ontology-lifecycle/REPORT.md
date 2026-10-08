# Ontology lifecycle report

Owner `eef831aa-dba4-4af2-a78b-541bc9e0a403`. Contract `oo.ontology-lifecycle/v1`.

OC-01 through OC-06 are not closed. `chain_closed` is false. `parent_acceptance` is false. `production_signed` is false. `certificate_class` is `not_issued`. A compiled pack is not a publication.

## What ran

Linux arm64 Rust 1.94.0, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, 6 GiB, 1 CPU, `--cap-drop ALL`, no new privileges, bridge network `oo-a403-eef831aa-net`, no host network, no socket, no host home. Pre-edit smoke exit 0, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, before `tools/ontology-lifecycle` existed. Focused tests and Clippy `-D warnings` exit 0 in `linux/focused-4/`. Ten tests passed: five library tests and five oracle tests. `children_spawned` is 0.

The service reads pinned producer files and returns 409. `POST` with `{"published":true}` is 400. `run --guard` leaves that directory empty. Exit code of `inspect` and `run` is 2 while the chain is open.

## Source pin

Requirements map head `fbd0467617a3c1514e7ffbef52f1138e20416ac6`. Pre-edit smoke head `b35ae9c5aae756e144a20499a9edf70a875fc11d`. Evidence observation head `b9f1e4fc30f144d1cf7dc61aa7765f58164a9caa`. The focused test compared file bytes, not the moving head. Those bytes still matched the pins in `REQUIREMENTS-MAP.json` for the docs, the self-build OpenAPI, curated Git apply, the release OpenAPI, `decide.rs`, and `pg-builder` `main.rs`. Agent-entry `CONTRACT.md` is still SHA-256 `3634c0a0f580770d5cf24d29d4e4c6d56f3028acfd9ddc0fbcb5554abafd06bf` and is not a git commit. Its marker is absent, so OC-05 is `absent`, not `unpinned`.

`crates/oo-builder/src/reviewed_effective.rs` is blob `7345b036685401d543e174a45a414e0e0db19613` at the smoke and map heads, and blob `b30207b6b3a0fec04a41b83663362889c44abaac` at the evidence head. This card did not execute either blob. The pre-commit recheck at `b9f1e4fc30f144d1cf7dc61aa7765f58164a9caa` repeated the source table with `/usr/bin/shasum -a 256`. The five producer markers were still absent as whole lines.

## What did not run

No private PostgreSQL. No 100000-row read. No model call. No `exportGeneratedOos`. No `reviews/decisions.yaml`. No curated overlay write. No Git commit of ontology. No G1–G13 evaluation. No active release. No agent session. Production external gates `credential`, `integrity`, `authenticated_owner`, `customer`, and `runsc` stay unmet. `production_admissible` in the pinned `decide.rs` returns false.

## Owned paths

`tools/ontology-lifecycle/**` and this evidence directory. No root manifest edit. No root lock edit. No edit to a373, a374, a392, a397, a402, the builder crates, or the frontend.
