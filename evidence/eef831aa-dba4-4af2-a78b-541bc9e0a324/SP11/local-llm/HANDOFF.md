# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a324`. Owner a324. Fixed archive `e8d297f95b32052ea587fe8ad67b6e8493a00300`.

## Owned paths

- `testing/real-db/local-llm/**` — contract, crate, lock, relay bin `oo-local-llm-relay`, models bin `oo-local-llm-models`.
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a324/SP11/local-llm/**` — preoverlay, proofs 1 through 11, host smoke and reachability, cleanup, this handoff.

No root `Cargo.toml` or `Cargo.lock` edit. No edit under `tools/ontology-dialogue` or `tools/description-provider`. The crate is a standalone workspace.

## What the next reader can run

The green Linux evidence is `linux/proof-10`: Rust 1.94.0, 16 tests passed, Clippy `-D warnings` exit 0, lock subset of the fixed archive. `linux/proof-11` built the Linux bins that the Docker probe used. Both the Linux target volume and the host Mach-O build were removed. A new listen also needs a new keypair. `host/ca.pem` and `host/leaf.pem` are public only.

To rebuild on Linux, use the pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, one builder, 6 GiB, 1 CPU, `--cap-drop ALL`, private cargo and target volumes, and `cargo test --locked --offline` plus `cargo clippy --locked --offline --all-targets -- -D warnings` against this crate. Do not run `cargo update` or `cargo generate-lockfile`. The lock is the proof-3 regenerate.

The relay command is `oo-local-llm-relay --listen 127.0.0.1:<port> --cert <leaf.pem> --key <leaf.key>`. It dials `127.0.0.1:8317` only. The models command is `oo-local-llm-models --origin <https-origin> --ca <ca.pem> [--bearer-file <path>] --out <path> [--chat-out <path>]`. The bearer file is read by the process. It is not an argument value.

Host loopback and container loopback differ. Preoverlay showed container `127.0.0.1:8317` closed and `host.docker.internal:8317` open. The relay process belongs on the host. A container client uses `https://host.docker.internal:<relay-port>` and the public CA. No host network, no Docker socket, no token mount, no key mount.

## Dialogue seam

`tools/ontology-dialogue` is absent from the fixed commit. It is present on the moving HEAD. a300 proof-8 reports `live_done=1` against archive `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`, which also lacks that path. a300's volumes are gone. The authorized token file `/tmp/oo-customer-demo-20261007/llm-token` is mode 600 and 6 bytes. Dialogue `read_bearer` requires 16 to 256 bytes, so a propose that points at this file returns `config` before a socket.

This packet's completed chain is the relay, one models discovery (HTTP 200, model `grok-4.3`), one chat status (HTTP 426, `candidate` false), and one Docker TLS probe (HTTP 401, no bearer). It does not include an operator approval file or a `query-pg` run. Model SQL was not executed.

`CONTRACT.md` still records the freeze-time a300 proof-7 seam. `FINDINGS.md` records the later seam. Leave the contract file unchanged if the proof-10 source hash must keep matching.

## Recipes

Origins in `linux/preoverlay/recipe-origins.txt`, bytes hashed when the preoverlay was written:

- a204 launch `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, sha256 `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`
- a204 inside, same commit, sha256 `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636`
- a200 launch `a587031c719a484830d78e493605f48e2056cae4`, sha256 `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163`
- a199 launch `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9`, sha256 `bf2a422d751db818cdd4d852d14be00436d4177cb584b85131a4774453413f55`

Changes from those recipes: memory 6g, cpus 1, global cap 10, no shared cargo volume, archive check against the description-provider seam, host-gateway reachability. The obsolete global cap of 6 was not used.

## Preserved failures

Keep `linux/proof` through `linux/proof-9`, `host/docker-reach.exit` (127), `host/smoke.exit` (1), and `host/offline-build.exit` (101). Passing counterparts are `linux/proof-10`, `linux/proof-11`, `host/reach.exit` (0), `host/docker-reach-2.exit` (0), `host/build.exit` (0), and `host/fetch.exit` (0).

## Cleanup already done

`CLEANUP.txt` and `CLEANUP.exit` 0. Removed: volumes `oo-a324-eef831aa-cargo` and `oo-a324-eef831aa-target`, network `oo-a324-eef831aa-net`, directory `/tmp/oo-a324-eef831aa-dba4-4af2-a78b-541bc9e0a324`. Token file retained. Recheck while writing this handoff: no `oo-a324` container, volume, or network; the tmp directory is absent; token metadata is still mode 600 and 6 bytes. No prune and no foreign container removal. Do not delete the token file from this unit.

## Inventory

`SHA256SUMS` covers both owned trees except `SHA256SUMS` and `SHA256SUMS.sha256`. `SHA256SUMS.sha256` is the digest of that list. Source blobs are the crate itself. `linux/proof-10/source-after.sha256` is the proof-time hash of those same files.

Parent acceptance, a full-goal close, and a release are out of scope for this handoff.
