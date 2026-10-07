# Local LLM relay

Session `eef831aa-dba4-4af2-a78b-541bc9e0a324`. Fixed commit `e8d297f95b32052ea587fe8ad67b6e8493a00300`, tree `265c9c6b16047292bfcca2f4a7d822bfec375e35`. Owned paths are `testing/real-db/local-llm/**` and this evidence directory. The root manifests, `tools/ontology-dialogue`, and `tools/description-provider` were not edited.

This packet is the host loopback TLS relay `oo-local-llm-relay` and the HTTPS models client `oo-local-llm-models`. Contract `oo.local-llm-relay/v1` is `testing/real-db/local-llm/CONTRACT.md`. It was frozen before the crate compiled. `linux/proof-10/source-after.sha256` still matches the crate byte for byte (`shasum -a 256 -c`, exit 0, this turn).

Preoverlay recorded repo head `e6ebf93b5ac261c097a24d145611e0300e4f88b5`. These reports were written while HEAD was `7d5e27c064182a05544eb83dae6c537e858d8e23`.

## What ran

The authorized backend is `http://127.0.0.1:8317`. The relay listens on `127.0.0.1:18317`, terminates TLS, and dials that address only. Allowlisted paths are `GET /v1/models` and `POST /v1/chat/completions`. Caps, the env refusal, and the CA versus leaf split are in the contract.

One host models call and one host chat call used the private token file. Recorded output:

| File | Result |
|---|---|
| `host/models.json` | HTTP 200, `selected_model_id` `grok-4.3`, `candidate_count` 18, `grammar_match_count` 18, `bearer_bytes` 6, `dialogue_bearer` `below_minimum`, `parse` `ok` |
| `host/chat.json` | HTTP 426, `candidate` false |
| `host/docker-models.json` | HTTP 401, `selected_model_id` null, counts 0, `bearer_bytes` null, `dialogue_bearer` `absent`, `parse` `fail` |

`host/models.stdout` is `oo-local-llm-models: ok`. `host/models.stderr`, `host/relay.stderr`, and `host/reach-relay.stderr` are empty. Relay stdout is `listen=127.0.0.1:18317` for both the smoke listener and the reachability listener. Token metadata in `host/token-meta.txt` is `mode=600 bytes=6`. The token bytes were not read into this report.

`host/smoke.exit` is 1. That file is the first Docker probe, which exited 127 because `/target/debug/oo-local-llm-models` was absent (`host/docker-reach.stderr`). `cargo test --all-targets` in proof-10 had left no bins. Proof-11 then built the bins (`phases/build.exit` 0). `host/reach.exit` is 0 and `host/docker-reach-2.exit` is 0. The 401 row is that second probe: the container trusted `host/ca.pem`, reached the host relay, and the relay reached the provider with no bearer. The token file and the private key were not mounted.

Public certificates, SAN `IP:127.0.0.1, DNS:host.docker.internal`:

- `host/ca.pem` `bfc36a694225909381ea8073a6a8d8ad774d3ab1a1c67e2626ea1ae9a5c8fadb`
- `host/leaf.pem` `6068773d7d8c70aa9bcb650d7149a9badd8534051b6a6a63e7e73e2e1906247a`

The matching private keys lived under `/tmp/oo-a324-eef831aa-dba4-4af2-a78b-541bc9e0a324/host/cert` and were removed with that directory. Those published certificates cannot listen again. A new listen needs a new keypair.

## Fixed-archive seam

`git cat-file -t e8d297f95b32052ea587fe8ad67b6e8493a00300:tools/ontology-dialogue` fails: the path is absent from that commit. `tools/description-provider` is a tree there. Preoverlay recorded the same split: dialogue absent, local-llm absent, description-provider present.

`tools/ontology-dialogue` is present at moving HEAD. a300's handoff names proof-8 `live_done=1` on archive `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`. That commit also lacks `tools/ontology-dialogue`. a300 containers and volumes were absent when this report was written. No compiled dialogue binary and no `query-pg` binary were available here. This packet did not start PostgreSQL and did not call `ontology-dialogue propose` or `execute`.

Current `tools/ontology-dialogue/src/propose.rs` `read_bearer` requires 16 to 256 visible ASCII bytes. The authorized token file is 6 bytes. A propose config that named that file would return `config` before a socket. The models client still sent one request and recorded `dialogue_bearer=below_minimum`.

`CONTRACT.md` still describes a300 proof-7 (`execute binary`, exit 101, no reports). That sentence is the freeze-time seam. It was left unchanged so `source-after.sha256` continues to match. The later a300 packet is a different archive and is not an input to this crate.

Model SQL was not stored and was not executed. The chat file records status and `candidate` only.

## Linux proofs

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, Rust 1.94.0 (`4a4ef493e 2026-03-02`), `aarch64-unknown-linux-gnu`. Each proof container was `oo-a324-proof` except preoverlay `oo-a324-preoverlay`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL, Privileged false, NetworkMode `oo-a324-eef831aa-net`, OOMKilled false. Each `container-removed.txt` names that container. New-builder policy was 6 GiB and 1 CPU. Global cap recorded in capacity files is 10. Proof-11 started with `global=1`.

| Directory | docker-run.exit | Stop |
|---|---|---|
| `linux/preoverlay` | 0 | probes: container `127.0.0.1:8317` exit 1, `host.docker.internal:8317` exit 0, LAN exit 1. OpenSSL 3.5.5. Container id `b00a5c6c346237849a1c26416612f3df1a2b3458b273cfdb318091cadbc14c37` |
| `linux/proof` | 1 | lock subset failed, 18 extra versions in `lock-extra.txt`. Container id `05cb2b5dc633b8ed444c61bb87448fa7c36b1b07802b62412849645bac032da8`. `lock-generate.exit` 0, `lock-generate-offline.exit` 101 |
| `linux/proof-2` | 101 | `update-js-sys.exit` 101. `wasm-bindgen-futures` 0.4.79 requires `js-sys` `=0.3.106`. Earlier precise updates exited 0 |
| `linux/proof-3` | 101 | `lock-generate.exit` 0, `lock_versions=subset`, `fetch.exit` 0. Compile: `E0658` match on `str` in `const fn`, `E0133` `remove_var` |
| `linux/proof-4` | 101 | `E0308` `windows` argument type |
| `linux/proof-5` | 101 | 10 passed, 6 failed. `InvalidCertificate(CaUsedAsEndEntity)` |
| `linux/proof-6` | 101 | 15 passed, 1 failed. `third_connection_is_busy` left 1 right 2 |
| `linux/proof-7` | 101 | tests 16 passed in 0.74s. Clippy exit 101: two `collapsible_if`, one `redundant_guards` |
| `linux/proof-8` | 101 | tests 16 passed in 0.63s. Clippy exit 101 on bin `oo-local-llm-models` |
| `linux/proof-9` | 101 | tests 16 passed in 0.71s. Clippy exit 101 on bin `oo-local-llm-relay`, `let_and_return` |
| `linux/proof-10` | 0 | tests 16 passed, 0 failed, 0.62s. `phases/test.exit` 0, `phases/clippy.exit` 0, `phases/rustup-clippy.exit` 0, `lock_versions=subset`. Clippy stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.42s` |
| `linux/proof-11` | 0 | `cargo build --locked --offline --bins`. `phases/build.exit` 0. Bins `oo-local-llm-models` 60079176 bytes and `oo-local-llm-relay` 59627696 bytes. Those files left with the target volume |

Failed proofs 1 through 9 stay in place. Proof-10 is the green Rust 1.94 test and Clippy run. Proof-11 only produced the Linux bins used by the second reachability probe.

## Host build

`host/capacity.txt`: own 0, global 0, host_cargo 0. `host/offline-build.exit` 101, `host/fetch.exit` 0, `host/build.exit` 0. `host/rustc-version.txt` is toolchain `host-default`, rustc 1.98.1 (`48a229cea 2026-09-01`), Homebrew, `aarch64-apple-darwin`. Rustup 1.94.0 was not installed on the host. The pinned Linux image cannot emit the Mach-O relay, and the private key stayed on the host. The Mach-O binaries were under the private tmp directory and were removed in cleanup.

## Recipes

`linux/preoverlay/recipe-origins.txt`:

```text
a204 launch b2b3adabd05de94723d8ed13b33be0144a0de1f9 08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841
a204 inside b2b3adabd05de94723d8ed13b33be0144a0de1f9 b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636
a200 launch a587031c719a484830d78e493605f48e2056cae4 837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163
a199 launch 5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9 bf2a422d751db818cdd4d852d14be00436d4177cb584b85131a4774453413f55
changes memory=6g cpus=1 global_cap=10 no_cargo_volume archive=provider_seam host-gateway
```

## Cleanup

`CLEANUP.exit` is 0. `CLEANUP.txt` records removal of volumes `oo-a324-eef831aa-cargo` and `oo-a324-eef831aa-target`, network `oo-a324-eef831aa-net`, and `/tmp/oo-a324-eef831aa-dba4-4af2-a78b-541bc9e0a324`. No owned container remained before that removal. After it, volume, network, and container listings for this owner were empty. The token file remained `mode=600 bytes=6`. This turn rechecked: no `oo-a324` container, volume, or network; the tmp directory is absent; the token file is still mode 600 and 6 bytes. No prune. No foreign cleanup.

## Inventory

`SHA256SUMS` lists every file under the two owned trees except itself and `SHA256SUMS.sha256`. `SHA256SUMS.sha256` is the digest of `SHA256SUMS`. Both are produced after this report.

This packet does not accept a parent ledger, a release, or a dialogue execution.
