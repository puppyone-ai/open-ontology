# Handoff

Verdict: **VERIFICATION_REPAIR_PRESENT**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a282`. Fixed spike `16cd808d5d4b69fed3b040b49047c7b67bfeeeb9`. Report HEAD `d9c5049757a40f16315152b3bfcbaff6ac5fc869`. Owned paths: `testing/spikes/p09-pi-rpc/src/drive.rs` and this directory. H-D01 remains pending parent acceptance. No product file, root manifest, root lock, ledger, or plan checkbox was edited.

The only source edit is the `clippy::collapsible_if` let chain in `drain_stderr`. Pi v1.0.4 commit `7c10bd4337495ee613f2224843ecdf349b80d1df` was driven again. `live.stdout` is `live_ok`.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `testing/spikes/p09-pi-rpc/PIN.md` and `CONTRACT.md` (unchanged)
- `linux/preedit-boundary/` for the 12 tests before the edit
- `linux/clippy-bridge/` for the real clippy failure, exit 101
- `linux/clippy-after-1/` for tests and clippy after the edit, both exit 0
- `linux/live-rerun/live/` for the rerun transcript, including `pi-version-pre.exit` 127
- `linux/hash-verify/` for the 473-entry compare
- `HASH-METHOD.txt`, `CLEANUP.txt`, `source-spike.sha256`
- `tools/sha256_manifest.rs` and `tools/manifest_eq.rs`

a279 `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md` stay the historical proof. This repair does not replace them.

## Decisive exits

`linux/clippy-after-1`: `test.exit` 0, `clippy.exit` 0, `docker-run.exit` 0. Command: `cargo clippy --locked --offline --all-targets -- -D warnings`.

`linux/live-rerun`: `live.exit` 0, `docker-run.exit` 0, network none. Container `bef1bf9cafeb47dcb6b1d5455362ba2d02ed83c68512d7a6dffc460276322b85`.

`linux/hash-verify`: `a279-compare.stdout` is `expected=473`, `fresh=474`, `bad=0`. `fixed-spike-compare.stdout` is `expected=25`, `fresh=25`, `bad=0`.

## Preserved failure

`linux/clippy-bridge` is the unmodified archive. `clippy.exit` 101. The log was not overwritten.

`linux/live-rerun/live/pi-version-pre.exit` 127 is the PATH miss on a direct `pi` invocation. The driver result is `version.exit` 0.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, `--pull never`, `--cap-drop ALL`, memory 6g, 1 CPU. One builder at a time. Private cargo and target volumes. Install clippy on a private bridge, then:

```text
cargo test --locked --offline -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

The live driver needs the Pi prefix and `mock_openai` from a private volume, then `--network none`:

```text
cargo run --offline --locked --bin pi-rpc-live -- <pi> <node-dir> <mock> <out>
```

Do not pass a production key. Do not add the spike to the root workspace. Do not `cargo update`. Do not promote Pi 1.0.4 to the root baseline.

## Cleanup

`CLEANUP.txt`. Own volumes, the private bridge, and `/tmp/oo-a282-eef831aa-dba4-4af2-a78b-541bc9e0a282` were removed. Each recorded exit is 0. `docker prune` was not run. Foreign resources were left in place.
