# Harness dependency delta

Status: `in_review`. Author session `eef831aa-dba4-4af2-a78b-541bc9e0a031`. The prior delta author was `eef831aa-dba4-4af2-a78b-541bc9e0a023`. This file is not a sign-off. `full_at_sec_05` is false. The security owner is not appointed, so 7-day exceptions are invalid.

`xtask` adds no registry crate. Direct edges are path crates `oo-core` and `oo-arch-check`, plus the existing pins jsonschema 0.57.0, serde 1.0.229, serde_json 1.0.151, sha2 0.10.9, and thiserror 2.0.21. No workspace feature was added. The root alias remains `run --locked -p xtask --`.

The bench generator is not a workspace member. A fixture receipt spawns an outside-repo `OO_XTASK_CARGO` with `--manifest-path`, `--locked`, `--offline`, and the operation flags `verify --dataset`, `generate --scale --seed --output --scope`, `export-sources --dataset --output`, or `list schema|fixtures`. Child nonzero and a missed deadline are exit 1. Child zero stays exit 3. A development receipt binds the a035 evidence and does not spawn from an author marker. W05 datagen integration is false.

## Tools, not lockfile members

| Tool | Version | Crate checksum | Published | Run |
| --- | --- | --- | --- | --- |
| cargo-audit | 0.22.2 | `700c2b240f7fd330c24b675fe429f73a5b676531fcc6300400b2b67f155ba12a` | 2026-06-05T12:33:04Z | exit 0 |
| cargo-deny | 0.20.2 | `e528dfcbe739af7ce37a77d3d6df1b29dd6887b1c701d888820c0f16b864f737` | 2026-07-09T17:58:09Z | exit 0 |

Both are older than 168 hours relative to the 2026-10-01 session date. Install uses a temporary cargo home. The tool tree is not folded into `Cargo.lock`. Both tools are Apache-2.0 OR MIT. License texts are under `supply-chain/licenses/harness/`.

Invocations that actually ran:

- `cargo-audit audit --json`
- `cargo-deny --format json --config ci/deny.toml --locked check`

`ci/deny.toml` sha256 `ee87f177fbf221221c5f3a7f0020dc393a047da9af77ec797c2953fc25136559`.

## What the scans found

cargo-audit 0.22.2 against advisory database commit `3461c0d8f85d084552dd999c58d97c7123a9e0fd` (commit time 2026-10-01T09:31:41+02:00) exited 0. The prior database commit recorded by a023 was `9b3a3b73a7f42606494c943e95f8196e9994df46`. cargo-deny 0.20.2 on the same `ci/deny.toml` bytes exited 0.

The root lock age walk on 2026-10-01 covered 119 registry packages, including the wasm graph. Young 0, yanked 0, checksum mismatches 0, index-cache misses 0. Floor is 604800 seconds. Tool age is not the graph age. Wasm packages are age-checked. Wasm is not a supported target matrix.

Five target-specific packages that were under 168 hours were replaced in the root `Cargo.lock` only, with compatible older releases already published before the floor: `js-sys` 0.3.105 and `wasm-bindgen`, `wasm-bindgen-macro`, `wasm-bindgen-macro-support`, `wasm-bindgen-shared` 0.2.128. The young versions and their checksums stay in `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a031/SP00/harness-corrections/red/hn13-lock-ages.txt`. `bench/datagen/Cargo.lock` was not rewritten.

cargo-deny 0.20.2 summary: advisories errors 0, bans errors 0 and warnings 1, licenses errors 0 and helps 135, sources errors 0.

The bans warning is duplicate crate `syn` 2.0.119 and `syn` 3.0.6. `multiple-versions` is `warn`. That warning does not close AT-SEC-05.

`borrow-or-share` 0.2.4 (jsonschema 0.57.0, referencing 0.57.0, fluent-uri 0.4.1) is SPDX `MIT-0` (OSI MIT No Attribution). It is an existing transitive crate. `ci/deny.toml` allows that SPDX id specifically. There is no clarify block and no exception object.

`chrono-tz` 0.10.4 was not rejected. No extra SPDX waiver was added for it.

Workspace path dependencies have no version field. cargo-deny classifies those as wildcards. `wildcards = "deny"` stays, and `allow-wildcard-paths = true` permits the private path edges. Registry `*` versions stay denied. Other crate manifests were not edited to invent versions.

## Scopes cargo does not cover

- JavaScript: `console/` and `sdk-ts/` are absent
- Native product code: `xtask` has no `build.rs`
- Image OS: pins exist at `testing/infra/images.lock.json` and `testing/infra/images.pins`. No OS scanner version is pinned, and no image scan ran

Machine-readable twin: `supply-chain/harness-dependency-delta.json`.
