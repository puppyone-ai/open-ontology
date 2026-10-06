# Handoff

Verdict: **APPROVE_LIMITED_DEV_ADAPTER**. The full harness stays open. H-D02 is not checked off. The parent harness is not accepted. No release is claimed.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a296`. Owned path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a296/SP10/pi-harness-review/**`. The product tree, spike, root manifests, root lock, plan, and ledger were not edited. Fixed archive `cc98bab7eba2c58c127435db4d2ae777797259de`.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `linux/preoverlay-msrv/` for the author suite before the overlay, including the failed source oracle
- `linux/oracle-2/` for the corrected source and registry oracle
- `linux/overlay/`, `linux/overlay-2/`, and `linux/overlay-3/` for the boundary tests
- `linux/pi-install/` for the pinned Pi 1.0.4 and Node v22.23.3 install
- `linux/pi-live/` for the mode passes and the `lo`-only oracle failure
- `linux/pi-live-2/` and `linux/pi-live-oracle/` for the network-none live proof
- `origin/INDEX.txt` and `RECIPE-CHANGES.txt`
- `MANIFEST.sha256` and `CLEANUP.txt`

## Decisive exits

`linux/preoverlay-msrv`: `test.exit` 0, `clippy.exit` 0, `registry-oracle.exit` 0, `source-oracle.exit` 1, `docker-run.exit` 1. Container `c71ef70ee2d9f7d4458c0903c43ed1c3a074422396284b75104b721af30cbdbd`.

`linux/oracle-2`: `registry-oracle.exit` 0, `source-oracle.exit` 0, `docker-run.exit` 0. Container `26d6f52ce084d428fee2bff8e15e02887e51c134253ad2b4d67dcbeb76345d7b`.

`linux/overlay-3`: `test.exit` 0, `clippy.exit` 0, `docker-run.exit` 0. Container `18ca4ae2551a90fcbb37ffd553b89b734a1e19d3ab40f9d84ad2e85608c08bd9`. Eleven boundary tests plus the author 5 and 17.

`linux/pi-install`: `inside.exit` 0, `pin-oracle.exit` 0. `pi-version.stdout` is `1.0.4`. `node-version.txt` is `v22.23.3`. Published day 2026-10-05. Run day 2026-10-06. Age 1. Container `16f3f8f6033da69c3c644bbfce8a07de91c59e5350dffa5b7b414d477b9c738c`.

`linux/pi-live-2`: `build.exit`, `tool.exit`, `abort.exit`, `disconnect.exit`, `restore.exit`, `live.exit`, and `docker-run.exit` are 0. `NetworkMode=none`. `Privileged=false`. Container `48ad79b22fd749cc5bc080d85c6448c7a645e0a702d41bc9f5ecb2c08835b348`.

`linux/pi-live-oracle`: `live-oracle.exit` 0 and stdout ends at `live_oracle_ok`. Container `45645a4243b9f72fdd9e794ae778b5a6dff61a209066922079fb1855fe061172`.

## Preserved failures

`linux/preoverlay-msrv` source oracle, `linux/overlay` compile, `linux/overlay-2` reorder assertion, and `linux/pi-live` interface oracle. Details and the corrected reruns are in `FINDINGS.md`. None of those consoles were overwritten.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `--pull never`, `--cap-drop ALL`, memory 6g, 1 CPU, one builder. Private names were `oo-a296-eef831aa-net`, `oo-a296-eef831aa-cargo`, `oo-a296-eef831aa-harness-target`, `oo-a296-eef831aa-pi-prefix`, and `oo-a296-eef831aa-npm-cache`.

Author suite, from `linux/preoverlay-msrv/inside.sh`:

```text
cargo fetch --locked
cargo test --locked --offline -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

Overlay, from `linux/overlay-3/inside.sh`, after the cargo volume already holds the lockfile fetch:

```text
cargo test --locked --offline -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

Live, from `linux/pi-live-2/inside.sh`, network none, prefix mounted read-only:

```text
cargo build --locked --offline --bin pi-harness-live --bin fixture-provider
```

Then `pi-harness-live` modes `tool`, `abort`, `disconnect`, and `restore`. The oracle is `tools/review_oracle.rs`, compiled with `rustc --edition 2021 -O` inside the image. `tools/sha256_manifest.rs` writes `MANIFEST.sha256`.

## Inventory and cleanup

`MANIFEST.sha256` is the SHA-256 of every regular file in this directory except that manifest. The method is `tools/sha256_manifest.rs` in the pinned image, network none. `linux/manifest/` is the first walk's copied console and hostconfig, and it is inside the final manifest. `linux/manifest-rehash/hostconfig.txt`, `docker-run.exit`, `container-removed.txt`, `tmpfs.txt`, and `console.log` are written after that final walk and sit outside `MANIFEST.sha256`. `CLEANUP.txt` lists the UUID-private containers, volumes, network, and `/tmp/oo-a296-eef831aa-dba4-4af2-a78b-541bc9e0a296` removed after the proof. The pinned image was left in place. `docker prune` was not run. Foreign containers were left untouched.
