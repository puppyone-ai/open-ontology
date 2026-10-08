# Source inventory

Phase read for the requirements map: git head `fbd0467617a3c1514e7ffbef52f1138e20416ac6`.

Pre-edit smoke head: `b35ae9c5aae756e144a20499a9edf70a875fc11d`. The owned tool path was absent. Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

Evidence observation head: `b9f1e4fc30f144d1cf7dc61aa7765f58164a9caa`. Producer file hashes below were unchanged from the focused test. The pre-commit recheck used `/usr/bin/shasum -a 256` at that same head and matched this table. Whole-line markers `operationId: exportGeneratedOos`, `operationId: exportReviewDecisions`, `operationId: compileEffectiveYaml`, `operationId: applyReviewedTree`, `operationId: evaluateGatesFromGit`, and `operationId: bindActiveReleasePack` were absent. Self-build `src/` was dirty in the worktree. The pinned OpenAPI file and `CONTRACT.md` bytes were not.

| Path | SHA-256 |
|---|---|
| docs/02-technology-stack.md | `c7bc07ca5937b425a757200a11f1a51f8b44478b9da4d25c057907f826e675b9` |
| docs/06-ontology-layer.md | `d224101642de6d0b36820ce9edf647b338ce1a0594c9cd3ac91fb9c5e9bdb749` |
| docs/08-agent-interface.md | `4006c07959d1366798b1325cae4b7743205f14202a23c71ed4dd4dd131a5ac34` |
| docs/11-deployment-and-operations.md | `1a60e23394e46db6e7dde4fff98022c331bb9a337f16c58e0ec588ef71a58f8f` |
| docs/12-configuration-reference.md | `ce0fd4080f6e7ae4ae8d36a4cfd82d9873034ae8dda50fd728c50dd0f45db810` |
| specs/open-ontology-v0.1-spec.md | `accac91210579da06387c94a28a6e71430a9c629cab9c6921c137d2e2f72fb91` |
| specs/open-ontology-v0.1-plan.md | `5d9d5e6585d7b220c0a4990d522b4ff771c5459de47eacf2551eaba79f8741e0` |
| specs/selfbuild-convergence-exit-plan.md | `746aaf9d5ee6bb19e976812a95c9a5469d15e717f5cd7825b96aa04b0be0fb2e` |
| tools/selfbuild-control-api/openapi/selfbuild-control-api.openapi.yaml | `4a7a155d801e47590f3c60927abf6035da4fc2ca118d318f7f8204330f149cf0` |
| tools/selfbuild-control-api/CONTRACT.md | `dda56df4c8a1ca751f7881951c15921b9cecbf3a9e30a7f7adc3be164d1a4ef9` |
| tools/curated-git-apply/CONTRACT.md | `4b9f84f013554cee19dcb40209181b0276135a88d098c7261331dc6a7fab740c` |
| tools/release-control-api/CONTRACT.md | `96a343d819cab557228fc802af34a9893a8a8ec1009dd161c84b2ae9c783d747` |
| tools/release-control-api/openapi/release-control-api.openapi.yaml | `de281a35aa6f6a75e2a6c4cb8e1a9fb3d2f76dc20799db164c024d1bd4239946` |
| tools/release-coordinator/src/decide.rs | `439aa2112925934c7f448f98a220fb889b708cb27fd1c7aa214b21a4d75496f4` |
| tools/agent-entry/CONTRACT.md | `3634c0a0f580770d5cf24d29d4e4c6d56f3028acfd9ddc0fbcb5554abafd06bf` |
| tools/pg-builder/src/main.rs | `75b6366e89218e19849e4f1812f7d922512f4ea91f420a34d904347f28cb98ea` |
| examples/schema/ontology.yaml | `eb71ad3ac338866360639a2e85a01ccd167b84833579ecc2698829c4cc151e5e` |
| examples/schema/decisions.yaml | `919e3786e67b8c171d0b9d485ee48a1336042d998fc5b194e7a8ba59854db505` |

At the smoke head and the requirements-map head, `crates/oo-builder/src/reviewed_effective.rs` is blob `7345b036685401d543e174a45a414e0e0db19613`, raw SHA-256 `189e8fb5e0d19e218ecdda625d8006e8b28fc295765f21106235aebb1e1daf4a`. At the evidence head `b9f1e4fc30f144d1cf7dc61aa7765f58164a9caa` the same path is blob `b30207b6b3a0fec04a41b83663362889c44abaac`, raw SHA-256 `d46c60c393e8c4f62e76a64ce2ec2f211bc290f25dfa4d9df9b27957ea6210b2`, and the worktree matches that blob. It is not a runtime pin and was not executed.

`crates/oo-core/src/oos/parse.rs` matches HEAD blob `edde650fa6bb77c5999e6204545139020c823a0d`. Its raw SHA-256 is `ee222973f1594634fca98ec6688eaa0e61fbef3ef217fec46cbefb4c49cb1674`. That hash is not a runtime pin.

Owned tool files are listed with hashes in `SHA256SUMS` after that file is generated. No root `Cargo.toml` or `Cargo.lock` change.
