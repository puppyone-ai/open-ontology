# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a233`. Reviewed commit `baf70c5c7b6d01183a178f53c77f65da4a7bb497`. This handoff does not accept M1-D14B. It does not check a parent box. It does not release. M1-D14A and SP06-W01 stay open.

## Verdict

`APPROVE_LIMITED` for `compile_reviewed_policies` and its two-role consumption of sealed reviewed-effective output at that commit. Evidence is this directory. Author proof was read at `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a227/SP06/reviewed-policy/`.

## Read first

- `REPORT.md` — smoke, overlay, commands, and raw exits.
- `FINDINGS.md` — producer bindings, boundary cases, and the `draft_hash` judgment.
- `linux/smoke/phases/` — author `reviewed_policy` tests, 21 passed, exit 0, before the harness existed.
- `linux/overlay/phases/` — nine independent tests and Clippy `-D warnings`, exit 0.
- `origin/recipes.txt` — a204, a200, and a199 blobs, plus the a227 libclang extract this crate needs.
- `CLEANUP.txt` — a233 volumes, network, and private tmp. Images kept.
- `linux/hash/hostconfig.txt` — the inventory container.

## Reproduce

One Linux arm64 Rust 1.94 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--memory 6g`, `--cpus 1`, private cargo and target volumes. Archive `baf70c5c7b6d01183a178f53c77f65da4a7bb497`. Move the extracted `rust-toolchain.toml` aside. No PostgreSQL.

Smoke, harness absent:

```text
cargo build --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos
cargo test --locked --offline -p oo-gateway --test reviewed_policy -- --test-threads=1
```

Set `OO_A227_OO_BUILDER` and `OO_A227_REVIEWED_EFFECTIVE` to those binaries.

Overlay, harness copied onto a fresh archive:

```text
cargo test --locked --offline -p oo-gateway --test reviewed_policy_a233_review -- --test-threads=1 --nocapture
cargo clippy --locked --offline -p oo-gateway --lib --test reviewed_policy --test reviewed_policy_a233_review -- -D warnings
```

Set `OO_A233_OO_BUILDER` and `OO_A233_REVIEWED_EFFECTIVE` to the bins built in that container. Green transcripts: `linux/smoke/console.log` and `linux/overlay/console.log`. `smoke.exit` is 0. `overlay.exit` is 0.

## Inventory

`linux/hash/launch.sh` compiles `tools/sha256.rs` with the pinned image and writes `SHA256SUMS` plus `SHA256SUMS.sha256`. The tool skips those two names. `linux/hash/hostconfig.txt` is written before the digest walk. The script removes only `oo-a233-hash`.

## Still open

Authentication, grants, publication, live row security, and a non-superuser grantor. Gateway contract step 7 names `provenance.draft_hash` on the draft. The producer and this adapter put that digest on `wrapper.draft_hash` and `command.provenance.draft_hash`. The contract file was not amended. No dependency was added.

## Cleanup

`oo-a233-smoke` and `oo-a233-overlay` were removed by their launch scripts. `cleanup/run.sh` removes any of those that remain, volumes `oo-a233-eef831aa-cargo` and `oo-a233-eef831aa-target`, network `oo-a233-eef831aa-net`, and `/tmp/oo-a233-eef831aa-dba4-4af2-a78b-541bc9e0a233`. The script checks owner `a233` and this session. It does not prune. The transcript is `CLEANUP.txt`. Pinned images stay. `linux/hash/launch.sh` removes its own container after the inventory.
