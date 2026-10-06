# S5-D01 selected_datasets order repair

## Result

The producer now sorts `selected_datasets` as dataset strings on every outcome. Source commit `56b791368d7b5ec050aa36ac91d33bc2234bc4d2`. Fixed archive `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a`, tree `525a432045d17087ee412be0d35550e94c55142a`. Workspace HEAD during the runs was `333406912ddeee60af04b7026d6231a465cd3dc2`. The producer file at that HEAD matched the fixed blob before the edit.

This is not parent acceptance, not full S5, and not a release. a257 stays REQUEST_CHANGES until an independent review of `56b79136`.

## Cause

`contracts/builder/description-candidate-v1.md` says `selected_datasets` is the admitted visible datasets, sorted. Cards are ordered by `(schema, name)`. Candidates are ordered by `dataset`.

Physical `A` becomes ident `q_41` and dataset `public.q_41`. Physical `b` stays `public.b`. `(schema, name)` puts `A` first. Dataset sort puts `public.b` first.

`selected_names` already sorted the dataset strings, and the early table-budget document used that order. `build_request` sorted tables by `(schema, name)` and stored that order in `Ready.selected_datasets`. The other outcomes copied that field, so they emitted `public.q_41` before `public.b`.

The edit sorts those dataset strings after the cards are built. It does not change card order, candidate order, the contract, lib exports, manifests, the lock, or the existing tests.

## How the bytes were built

Smoke, red, proof, and CLI used `git archive fbc10b04` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts`. The archive mount was read-only. The container copied it and moved `rust-toolchain.toml` aside. `RUSTUP_TOOLCHAIN=1.94.0`. `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. `cargo update` was not run.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Each builder had 6 GiB, 1 CPU, `--cap-drop ALL`, `--security-opt no-new-privileges`, `--pull never`, private bridge `oo-a262-eef831aa-net`, `HOME=/tmp`. No PostgreSQL. No model call. No host cargo process. Capacity at each launch was `own=0 global=0 host_cargo=0`.

Recipe origins and the hashes this repair copied are in `ORIGIN.md`. The a199 manifest tool and the a257 file hasher were copied unchanged.

## Smoke

`linux/smoke` ran before any overlay. `docker-run.exit` 0. `smoke.exit` 0. Container `14c34807829e47473082d73de96bf02e6b2d22b192c9879cd474ad1e491038f3` removed. OOMKilled false. Privileged false. Memory 6442450944. NanoCpus 1000000000. NetworkMode `oo-a262-eef831aa-net`.

- offline fetch exit 101, then locked fetch exit 0
- lib `description_candidate`: 2 passed, 0 failed
- integration `description_candidate`: 10 passed, 0 failed
- overlay absent, including the new test and the description CLI
- tree diff is only `rust-toolchain.toml` moved aside
- lock unchanged
- unfixed producer SHA-256 `68f46e95307efc403164bb71f6cc2a37cd5103c602abd36a9d6b68a9271ea007`

Smoke did not run Clippy.

## Red

`linux/red` copied only `description_selection_order.rs` onto the unfixed tree. `phases/test-order.exit` 101. `red.exit` 0. `docker-run.exit` 0. Container `f9a874a2eccf7335d29f60d597f737261585027ee31f76ad353dc5710576df17` removed. Same memory, CPU, capability, and network limits. OOMKilled false. Privileged false.

The new test compiled a captured catalog of physical tables `A` and `b`, selected `b` then `A`, and called the public prepare and complete functions. The failure was:

```
dataset-order mismatches: success=["public.q_41","public.b"] | truncated=["public.q_41","public.b"] | incomplete=["public.q_41","public.b"] | provider_failure=["public.q_41","public.b"] | format=["public.q_41","public.b"] | response-budget=["public.q_41","public.b"]
```

Lib tests 2 passed. Author tests 10 passed. Lock unchanged. Tree diff is the new test plus the toolchain file moved aside.

## Proof

`linux/proof` copied the patched producer and the same test. `proof.exit` 0. `docker-run.exit` 0. Container `3b5228e2bc26550d638ad9657473dda4f9f2d1143bac1ea1e4fec3537d3781fb` removed. OOMKilled false. Privileged false. Memory 6442450944. NanoCpus 1000000000.

- lib `description_candidate`: 2 passed
- author integration: 10 passed
- `description_selection_order`: 1 passed
- Clippy component installed with `rustup component add clippy --toolchain 1.94.0`, exit 0
- `cargo clippy -D warnings` on the lib and those two tests, exit 0
- lock unchanged
- patched producer SHA-256 `c392dd9ef7dfff45ea9f6298b257e8fc4bdeca04784acbfbb696ef644f884954`
- new test SHA-256 `449b6db74c39e169fdd79b608538233ff2de9eab683b63217ac6a8b5c3d8b768`
- tree diff is the producer file, the new test, and the toolchain file moved aside

The passing test keeps cards at `A` / `public.q_41` then `b` / `public.b`, candidates at `public.b` then `public.q_41`, and `selected_datasets` at `public.b` then `public.q_41` for success, truncated, incomplete, provider failure, duplicate-key format rejection, response-byte budget, and the early table budget.

## CLI

The fixed archive has no `description-candidate` binary. `linux/cli` overlaid the unchanged source from `827e768b9f94af6134a5734bd16e066ab0d47528` and did not edit it. `cli.exit` 0. `docker-run.exit` 0. Container `9d307d9b14001ec7ba61ce078381ea0b658e244e854017ba7ff77ea0dbb014e0` removed. Same limits. OOMKilled false. Privileged false.

The order test passed again and wrote the library documents. `cargo build --bin description-candidate --locked --offline` exited 0. The binary's stdout matched those documents for `prepare_request` (early budget and the full request) and `complete_candidates` (success, truncated, incomplete, provider failure, format, response budget). The success stdout has `selected_datasets` and `candidates` both `public.b` then `public.q_41`. The early-budget stdout is `budget_failure` / `tables` with the same dataset list and a null `request_sha256`. Lock unchanged.

## Cleanup

Own volumes `oo-a262-eef831aa-cargo` and `oo-a262-eef831aa-target`, network `oo-a262-eef831aa-net`, and `/tmp/oo-a262-eef831aa-dba4-4af2-a78b-541bc9e0a262` were removed after the CLI run. No prune and no foreign cleanup. See `CLEANUP.txt`.
