# Handoff

Verdict: **ADAPTER_DEV_PRESENT**. H-D02 is not accepted. The parent harness is not accepted. No release is claimed.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a287`. Owned paths: `tools/pi-harness/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a287/SP10/pi-harness/**`. The spike, root manifests, root lock, plan, and ledger were not edited.

## Read first

- `tools/pi-harness/CONTRACT.md` for the frozen API and the mapping filled from the capture
- `REPORT.md` and `FINDINGS.md`
- `linux/preedit-boundary/` for the 12 RPC tests before adapter edits
- `linux/focused-1` through `linux/focused-4` for preserved compile and test failures
- `linux/focused-6/` for the passing stub tests and clippy
- `linux/pi-install-1/` for the pinned Pi 1.0.4 and Node 22.23.3 install
- `linux/pi-capture-1/out/capture.ndjson` for the raw tool records
- `linux/pi-live-1/` for tool, abort, disconnect, and restore
- `MANIFEST.sha256`, `source-pi-harness.sha256`, `recipe-origin.sha256`, `HASH-METHOD.txt`

## Decisive exits

`linux/focused-6`: `test.exit` 0, `clippy.exit` 0, `docker-run.exit` 0. Container `75a2c1b4d5c4f2b5222412d4fbbfff8a69bd94d98fef43a26445b8f9b8a597f7`. Five unit tests and 17 integration tests. Clippy was `cargo clippy --locked --offline --all-targets -- -D warnings` after `components-after.txt` showed the clippy component.

`linux/pi-live-1`: `tool.exit`, `abort.exit`, `disconnect.exit`, and `restore.exit` are 0. `docker-run.exit` 0. Network mode `none`. Container `a2e82a5bd9aa9add773b6d8e1bd109153f82ecfd7c0e6eb8ee0341a7abb9c460`.

`linux/pi-install-1`: `inside.exit` 0, `pin-sha.exit` 0, `pi-version.stdout` is `1.0.4`, `node-version.txt` is `v22.23.3`, git commit `7c10bd4337495ee613f2224843ecdf349b80d1df`.

## Preserved failure

`linux/pi-capture-1` `docker-run.exit` is 1 and `capture.stderr` says the probe did not observe `agent_settled`. `out/capture.ndjson` line 31 is that record. The file was not rewritten. Later mapping and `linux/pi-live-1` use those bytes.

`linux/focused-1` through `linux/focused-4` keep their `console.log` and exit 101 logs.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, `--pull never`, `--cap-drop ALL`, memory 6g, 1 CPU. One builder at a time. Private cargo volume `oo-a287-eef831aa-cargo` and target volume `oo-a287-eef831aa-harness-target` until cleanup. Install clippy on the private bridge, then offline:

```text
cargo test --locked --offline -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

The live runs need the prefix from `linux/pi-install-1` mounted read-only, then `--network none`:

```text
pi-harness-live tool <pi-cli.js> <node> <out>
pi-harness-live abort <pi-cli.js> <node> <out>
pi-harness-live disconnect <pi-cli.js> <node> <out>
pi-harness-live restore <pi-cli.js> <node> <out>
```

The executable path must be the real `dist/bundle/cli.js`, not the `node_modules/.bin/pi` symlink. Do not pass a provider key. The apiKey placeholder is `public-proxy-marker`. Do not add this crate to the root workspace. Do not `cargo update`. Do not promote Pi 1.0.4.

## Cleanup

`CLEANUP.txt`. Own a287 volumes, network `oo-a287-eef831aa-net`, and `/tmp/oo-a287-eef831aa-dba4-4af2-a78b-541bc9e0a287` are removed. `docker prune` is not run. Foreign resources stay.
