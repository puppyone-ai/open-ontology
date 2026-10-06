# Handoff

Verdict: **PROOF_PRESENT_ACCEPTANCE_PENDING**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a279`. Fixed baseline `7f500ff0a61a42ea606a281dd931099091a52ef6`. Report HEAD `41d47faf5badbc72ba99c7f5b39acbde0b5fcb08`. Owned paths: `testing/spikes/p09-pi-rpc/` and this directory. H-D01 remains pending parent acceptance. No product file, root manifest, root lock, ledger, or plan checkbox was edited. a207 remains the connector manifest and root-lock writer.

Pi v1.0.4 commit `7c10bd4337495ee613f2224843ecdf349b80d1df` was driven over real stdio. The stdin commands `export` and `restore` return `Unknown command`. Transcript is `get_messages` plus the session `.jsonl`. Restore is `switch_session` with `data.cancelled` false, then `get_messages`. The local mock is Safe Rust on `127.0.0.1` with token `public-dummy-fixture-token` and reply `fixture-reply`.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `testing/spikes/p09-pi-rpc/PIN.md` and `CONTRACT.md`
- `linux/adapter-proof-4/` for the decisive run, including `live/` and `test.stdout`
- `linux/preedit/` for the MSRV smoke before the driver
- `linux/adapter-proof/`, `adapter-proof-2/`, and `adapter-proof-3/` for preserved driver failures
- `linux/pi-install/`, `pi-live-abort/`, `pi-live-restore/`, and `pi-live-restore-switch/` for earlier preserved failures
- `CLEANUP.txt`, `HASH-METHOD.txt`, `MANIFEST.sha256`, `source-spike.sha256`
- `tools/sha256_manifest.rs` and `tools/ORIGIN.txt`

## Decisive exits

`linux/adapter-proof-4`: `test.exit` 0, `clippy.exit` 127, `live.exit` 0, `docker-run.exit` 0. Console: `test=0 clippy=127 live=0`. `live.stdout`: `live_ok`. `version.stdout`: `1.0.4`. Container `b9880feffa0561baf10109a8240760c883f08c33849e3ffae0672f7b545de9c2`.

`test.stdout`: `ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s`.

`linux/preedit`: `docker-run.exit` 0, `PHASE rustc_exit=0`, `PHASE smoke_exit=0`. Container `3001e47458034c3e07680a57350d7d717fe6e9b91beac4c74b20b51cfd54f02e`. Rust `1.94.0` on `linux/arm64`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

Clippy did not run. The image toolchain has no clippy component, and the proof network is `none`.

## Preserved attempts

- `pi-install`: `docker-run.exit` 2. `tar` cannot chown uid 1001. Fixed later with `tar --no-same-owner`.
- `pi-install-2`: `docker-run.exit` 1. Published npm package has no `LICENSE`. Git tag MIT text is `PI-LICENSE`.
- `pi-live-abort`: predicate matched the user prompt. Assistant content empty.
- `pi-live-restore`: concurrent commands, no `sw1` response, `get_messages` returned `messages []`.
- `pi-live-restore-switch`: observe exit 126, tmpfs `noexec`.
- `adapter-proof` (same container as `pi-live-restore-switch-2`, id `62a18417e288ed294a5bd5c902fc73e33fccf0a94cf4cbb251dfec50a2253ee8`): `test.exit` 101, `live.exit` 101, `clippy.exit` 1. `map_err(ToString::to_string)` did not type-check. Console on the restore-switch-2 side ends `PHASE test_exit=1`. The wrapper `docker-run.exit` 0 is not a pass.
- `adapter-proof-2`: tests passed, live exit 1, `--mode` checked with a 5-byte window.
- `adapter-proof-3`: tests passed, live exit 1, `"cancelled":false` checked with a 16-byte window. `switch.json` still holds the successful response.

Fifteen unique container ids. Cap was 12. The thirteenth through fifteenth ids are `adapter-proof-2`, `adapter-proof-3`, and `adapter-proof-4`. No further container was started for the hash inventory.

## Re-run

Pinned image above, `RUSTUP_TOOLCHAIN=1.94.0`, `--pull never`, `--cap-drop ALL`, memory 6g, 1 CPU, private cargo and target volumes, `--network none` for the live driver. Mount the spike at `/work`. The Pi prefix and the `mock_openai` binary have to be rebuilt in a private volume. Node v22.23.3 linux-arm64 and `@earendil-works/pi-coding-agent@1.0.4` must match `PIN.md`. Do not pass a production key. Do not add the spike to the root workspace. Do not `cargo update`.

```text
cargo test --offline --locked -- --test-threads=1
cargo run --offline --locked --bin pi-rpc-live -- <pi> <node-dir> <mock> <out>
```

Clippy requires a toolchain that already contains the component. `rustup component add` needs a network this proof does not allow.

## Manifest and cleanup

`MANIFEST.sha256` is host `shasum -a 256` of every regular file under this directory except the manifest itself. The a204 Rust manifest tool is copied at `tools/sha256_manifest.rs` and was not executed. `source-spike.sha256` lists `testing/spikes/p09-pi-rpc`.

`CLEANUP.txt` records removal of the five `oo-a279-eef831aa-*` volumes, network `oo-a279-eef831aa-net`, and the UUID tmp directory. Each recorded exit is 0. `docker prune` was not run. PostgreSQL was not started. Foreign resources were left in place.
