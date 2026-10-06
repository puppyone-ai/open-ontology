# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a285`. Sealed spike `c551afea55d5a9164760878a5d6a76835b869875`. Report HEAD `d738a621c296e97458e2b9dd30947dd03bc1689b`. Owned path: this directory. H-D01 remains pending parent acceptance. No product file, root manifest, root lock, ledger, or plan checkbox was edited. `NO_OVERLAY`.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `testing/spikes/p09-pi-rpc/PIN.md` and `CONTRACT.md` (unchanged)
- `linux/preoverlay-msrv/` for the 12 tests on the sealed archive, network none
- `linux/preoverlay-clippy/` for the real clippy download and `clippy.exit` 0
- `linux/pi-reinstall/` for the preserved curl 35
- `linux/pi-reinstall-2/` for `pin_ok`, Node v22.23.3, and Pi 1.0.4
- `linux/live/live/` for the fresh transcript, including `pi-version-pre.exit` 127
- `HASH-METHOD.txt`, `CLEANUP.txt`, `linux/manifest/recipe-origin.txt`
- `tools/sha256_manifest.rs`, `tools/manifest_eq.rs`, and `tools/review_oracle.rs`

a279 and a282 stay the historical proof. This review does not replace them. a279 `clippy.exit` 127 and the a282 compare `expected=473 fresh=474 bad=0` were content-checked and left in place.

## Decisive exits

`linux/preoverlay-msrv`: `test.exit` 0, `docker-run.exit` 0, `worst.exit` 0. Result `ok. 12 passed; 0 failed; 0 ignored`. Container `ea74ab0afd4ad22dbb638ffc84be2e5140941e8f0ad5a58c06c5e442fb7d6157`.

`linux/preoverlay-clippy`: `clippy-install.exit` 0, `clippy.exit` 0, `docker-run.exit` 0. Command: `cargo clippy --locked --offline --all-targets -- -D warnings`.

`linux/pi-reinstall-2`: `pin-oracle.stdout` ends `pin_ok`. `pi-version.stdout` is `1.0.4`.

`linux/live`: `live.exit` 0, `docker-run.exit` 0, network none. `live-oracle.stdout` ends `live_oracle_ok`. Container `99caa3080013c839e4e5dc9189b571af96ddcc1b365a091c91b94e2b1c5c28e5`.

## Preserved failure

`linux/pi-reinstall/docker-run.exit` is 35. The console ends at the npm tarball with curl 35, TLS unexpected EOF. The retry is `linux/pi-reinstall-2` and was not written over the failed console.

`linux/live/live/pi-version-pre.exit` 127 is the PATH miss on a direct `pi` invocation. The driver result is `version.exit` 0.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, `--pull never`, `--cap-drop ALL`, memory 6g, 1 CPU. One builder at a time. Private cargo and target volumes. Install clippy on a private bridge, then on the sealed archive:

```text
cargo test --locked --offline -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

The live driver needs the Pi prefix and `mock_openai` from a private volume, then `--network none`:

```text
cargo run --offline --locked --bin pi-rpc-live -- <pi> <node-dir> <mock> <out>
```

Do not pass a production key. Do not add the spike to the root workspace. Do not `cargo update`. Do not promote Pi 1.0.4 to the root baseline. Literal `export` and `restore` stay unknown commands.

## Cleanup

`CLEANUP.txt`. Own volumes, the private bridge, and `/tmp/oo-a285-eef831aa-dba4-4af2-a78b-541bc9e0a285` are removed before the inventory. The manifest console directory is created again for the two hash passes and removed by pass 2. That removal is `outside_stdout_dir_removed` in `linux/manifest/rewrite-stdout.txt`, which is written after `MANIFEST.sha256`. `docker prune` was not run. Foreign resources were left in place.
