# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a239`. Reviewed commit `bcc8673463e8191f2cb9e8efb34ccc2c73574c8d`. This handoff does not accept M1-D14C. It does not check a parent box. It does not release. SP06-W01 stays open.

## Verdict

`APPROVE_LIMITED` for the `reviewed-policy` command and its two-role consumption of sealed reviewed-effective output at that commit. Evidence is this directory. Author proof was read at `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a234/SP06/reviewed-policy-cli/`.

## Read first

- `REPORT.md` — smoke, overlay, commands, blob ids, and raw exits.
- `FINDINGS.md` — clinic bindings, redacted refusals, the local DTO, and the 1 MiB envelope.
- `linux/smoke/phases/` — author `reviewed_policy_cli` tests, 6 passed, exit 0, before the harness existed.
- `linux/overlay/phases/` — five independent tests and Clippy `-D warnings`, exit 0.
- `origin/recipes.txt` — a204, a200, a199, and a233 launch blobs, plus the a234 libclang extract this crate needs.
- `CLEANUP.txt` — a239 volumes, network, and private tmp. Images kept.
- `linux/hash/hostconfig.txt` — the inventory container.
- `source/blobs/` — product files taken from the fixed commit, not from the worktree.

## Reproduce

One Linux arm64 Rust 1.94 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--memory 6g`, `--cpus 1`, private cargo and target volumes. Archive `bcc8673463e8191f2cb9e8efb34ccc2c73574c8d`. Move the extracted `rust-toolchain.toml` aside. No PostgreSQL.

Smoke, harness absent:

```text
cargo build --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos
cargo build --locked --offline -p oo-gateway --bin reviewed-policy
cargo test --locked --offline -p oo-gateway --test reviewed_policy_cli -- --test-threads=1
```

Set `OO_A234_OO_BUILDER`, `OO_A234_REVIEWED_EFFECTIVE`, and `OO_A234_REVIEWED_POLICY` to those binaries.

Overlay, harness copied onto a fresh archive:

```text
cargo test --locked --offline -p oo-gateway --test reviewed_policy_a239_review -- --test-threads=1 --nocapture
cargo clippy --locked --offline -p oo-gateway --bin reviewed-policy --test reviewed_policy_cli --test reviewed_policy_a239_review -- -D warnings
```

Set `OO_A239_OO_BUILDER`, `OO_A239_REVIEWED_EFFECTIVE`, and `OO_A239_REVIEWED_POLICY` to the bins built in that container. Green transcripts: `linux/smoke/console.log` and `linux/overlay/console.log`. `smoke.exit` is 0. `overlay.exit` is 0. No retry directory exists.

## Inventory

`linux/hash/launch.sh` compiles `tools/sha256.rs` with the pinned image and writes `SHA256SUMS` plus `SHA256SUMS.sha256`. The tool skips those two names. `linux/hash/hostconfig.txt` is written before the digest walk. The script removes only `oo-a239-hash`.

## Still open

Authentication, grants, publication, live row security, and a non-superuser grantor. The command output is a private local DTO. `compile_column_acl_set` is not a crate-root export. One operator document is capped at 1048576 bytes, so three adapter documents near that size do not fit. Gateway contract step 7 names `provenance.draft_hash` on the draft. The producer and the adapter reviewed at a233 put that digest on `wrapper.draft_hash` and `command.provenance.draft_hash`. This clinic artifact shows the same absence on `draft.provenance`. The contract file was not amended. No dependency was added.

## Cleanup

`oo-a239-smoke` and `oo-a239-overlay` were removed by their launch scripts. `cleanup/run.sh` removes any of those that remain, volumes `oo-a239-eef831aa-cargo` and `oo-a239-eef831aa-target`, network `oo-a239-eef831aa-net`, and `/tmp/oo-a239-eef831aa-dba4-4af2-a78b-541bc9e0a239`. The script checks owner `a239` and this session. It does not prune. The transcript is `CLEANUP.txt`. Pinned images stay. `linux/hash/launch.sh` removes its own container after the inventory.
