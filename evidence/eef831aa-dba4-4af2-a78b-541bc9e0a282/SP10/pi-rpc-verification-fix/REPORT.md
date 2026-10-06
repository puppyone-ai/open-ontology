# Report

Verdict: **VERIFICATION_REPAIR_PRESENT**. H-D01 is not accepted.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a282`. Fixed spike commit `16cd808d5d4b69fed3b040b49047c7b67bfeeeb9`. Repo HEAD when this report was written: `d9c5049757a40f16315152b3bfcbaff6ac5fc869`. Date: 2026-10-06.

Owned paths are this directory and `testing/spikes/p09-pi-rpc/src/drive.rs`. The a279 evidence tree, `PIN.md`, and `CONTRACT.md` were not edited. Root `Cargo.toml`, root `Cargo.lock`, the plan, and the ledger were not edited. a207 remains the connector manifest and root-lock writer. This session is the author record. It is not an independent approval and it does not accept H-D01.

There is no repository `AGENTS.md`. The harness text in `docs/09-sandbox-and-agent-runtime.md` section 7 is a template, not a file this repair edits. The read set was that section 6 contract, ADR-009, plan P9 and SP10-W04, the spike `CONTRACT.md` and `PIN.md`, and the full a279 `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`.

## What changed

`cargo clippy --locked --offline --all-targets -- -D warnings` on Rust 1.94.0 reported one warning, `clippy::collapsible_if` in `drain_stderr`. The nested lock check is now a let chain. The mutex guard and the 262144-byte cap are the same. No other spike file changed. `source-spike.sha256` is the Rust inventory of the edited spike. Against the fixed commit, the only differing path is `./src/drive.rs`.

| drive.rs | SHA-256 |
| --- | --- |
| fixed `16cd808d` | `67055b72202cb151b0f4c488425af2b906d9c87bae56631ee57ad1576514dbb0` |
| this edit | `03f253fd260faf7f293c329a42450b26c40582f2964760c29ed782db4314e4c1` |

Because that file is compiled into `pi-rpc-live`, the same Pi v1.0.4 pin was driven again. The live stdout is `live_ok`.

## Image

`docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `--pull never`. Rust `1.94.0` (`4a4ef493e 2026-03-02`). `RUSTUP_TOOLCHAIN=1.94.0`. Each new container was 6 GiB and 1 CPU. PostgreSQL was not started. No two builders ran at once.

The clippy containers used private bridge `oo-a282-eef831aa-net` (`driver=bridge`, `internal=false`) so `rustup component add clippy --toolchain 1.94.0` could download the component. `cargo test` and `cargo clippy` stayed `--locked --offline`. The pre-edit test, the live rerun, and the hash check used `--network none`.

## Exits

| Dir | Container | docker-run | What the exits say |
| --- | --- | --- | --- |
| `linux/preedit-boundary` | `0c846f8a5b81352554d29c5015c3bf4c2323b57e90581f95be08873e27a8a420` | 0 | Before the edit. `test.exit` 0. 12 passed, 0 failed, 0.45s |
| `linux/clippy-bridge` | `68f49dc67105303d2a390d3648e96d6c497222d104328f5ac05bc6776a6aa6ec` | 101 | Install exit 0. Real `clippy.exit` 101. Not a substituted 127 |
| `linux/clippy-after-1` | `71f0343f78a82adf4801b9bd2b33fdd2786822237d6ffcb45baf10aa876e5618` | 0 | After the edit. `test.exit` 0, 12 passed, 0.43s. `clippy.exit` 0 |
| `linux/pi-reinstall` | `a941aacc07a992ac1fb9e25a9fa8ed3e581d1ea93c79bd54b8a0c3ff1939e657` | 0 | Node v22.23.3, Pi commit `7c10bd4337495ee613f2224843ecdf349b80d1df`, `pi --version` `1.0.4`, npm ci 0, mock rustc 0 |
| `linux/live-rerun` | `bef1bf9cafeb47dcb6b1d5455362ba2d02ed83c68512d7a6dffc460276322b85` | 0 | `live.exit` 0, stdout `live_ok`, network none |
| `linux/hash-verify` | `66c984e3add245f7b9791917b24ccee702ba1be4c83ee97ff574df71903fda3d` | 0 | 473 original manifest entries bad 0. Fixed spike 25 bad 0 |

`linux/preedit-boundary/hostconfig.txt` and the later hostconfig files record `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`.

## Live pin

The rerun used the same pin as a279. `linux/pi-reinstall/pi-git-commit.txt` is `7c10bd4337495ee613f2224843ecdf349b80d1df`. `pi-version.stdout` is `1.0.4`. `node-version.txt` is `v22.23.3`. `npm-tarball.integrity` matches the integrity in `PIN.md`. The published npm package still has no `LICENSE` file. That absence is `installed-license-absent.txt`. The git tag MIT text was checked by the install script. v1.0.4 was published on 2026-10-05, so it stays under the seven-day floor and is not promoted to the root baseline.

`linux/live-rerun/live/pi-version-pre.exit` is 127. That pre-check invoked `pi` without Node on `PATH` (`env: 'node': No such file or directory`). It is not a clippy result and it is not the driver result. The driver sets `PATH` to the Node directory. Its `version.exit` is 0 and `version.stdout` is `1.0.4`. `help.exit` is 0.

| Step | Saved result |
| --- | --- |
| cold `export` | byte match with `fixtures/cold_export.json` |
| cold `restore` | byte match with `fixtures/cold_restore.json` |
| abort response | byte match with `fixtures/abort_response.json` |
| abort `get_messages` | assistant text `fixture-reply`, `stopReason` `aborted`, `errorMessage` `Request was aborted` |
| session file | assistant text `fixture-reply`, `stopReason` `stop`. The dummy token string is absent |
| `switch_session` | byte match with `fixtures/restore_switch_session.json`, `cancelled` false |
| restored `get_messages` | assistant text `fixture-reply`, `stopReason` `stop` |

`export` and `restore` remain unknown commands. Transcript bytes are `get_messages` and the session file. Restore bytes are `switch_session`, then `get_messages`.

## Hash inventory

`tools/sha256_manifest.rs` is a byte copy of the a204 tool. The Rust recheck of the origin file is `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. The copy compares equal. The tool's empty and `abc` digests match the FIPS vectors. `manifest_eq` compared the a279 `MANIFEST.sha256` (473 lines) with a fresh walk (474 lines, the extra line is `MANIFEST.sha256` itself). `bad=0`. The same compare of a279 `source-spike.sha256` against a git archive of `16cd808d` is 25 lines, `bad=0`. The a279 tree was mounted read-only and was not modified.

`MANIFEST.sha256` in this directory is produced after this report, by the container under `linux/manifest`. `HASH-METHOD.txt` records the method.

## Cleanup

`CLEANUP.txt`. Five `oo-a282-eef831aa-*` volumes and network `oo-a282-eef831aa-net` were removed, each exit 0. The private tmp directory exit is 0. `docker prune` was not run. Foreign containers were left in place. The pinned Rust image was left in place.

## Claims withheld

This record does not accept H-D01. It does not claim parent acceptance, a full goal, or a release. It does not implement the async product `HarnessAdapter`. It does not emit tool, clarification, or artifact events. It does not claim a sandbox, runsc, or product network isolation. `--network none` on the live rerun is the container setting for that run. The fifteen a279 containers remain historical evidence. They were sequential runs, and this repair did not treat them as a cap breach.
