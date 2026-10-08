# Source inventory

Recorded at source head `9c084b58d65f63229355fe6c20fd2f0285d6f29a`. Hashes are SHA-256 of the file bytes.

## Documents, unchanged this phase

| Path | SHA-256 |
|---|---|
| docs/02-technology-stack.md | `c7bc07ca5937b425a757200a11f1a51f8b44478b9da4d25c057907f826e675b9` |
| docs/06-ontology-layer.md | `d224101642de6d0b36820ce9edf647b338ce1a0594c9cd3ac91fb9c5e9bdb749` |
| docs/08-agent-interface.md | `4006c07959d1366798b1325cae4b7743205f14202a23c71ed4dd4dd131a5ac34` |
| docs/11-deployment-and-operations.md | `1a60e23394e46db6e7dde4fff98022c331bb9a337f16c58e0ec588ef71a58f8f` |
| docs/12-configuration-reference.md | `ce0fd4080f6e7ae4ae8d36a4cfd82d9873034ae8dda50fd728c50dd0f45db810` |
| specs/open-ontology-v0.1-spec.md | `accac91210579da06387c94a28a6e71430a9c629cab9c6921c137d2e2f72fb91` |
| specs/open-ontology-v0.1-plan.md | `6b1b1426af0f0b2290410640c8959987826f0c5c9a7ae989842215db3a423887` |
| specs/selfbuild-convergence-exit-plan.md | `746aaf9d5ee6bb19e976812a95c9a5469d15e717f5cd7825b96aa04b0be0fb2e` |

## Producer pins used for classification

| Path | SHA-256 | Classification |
|---|---|---|
| tools/selfbuild-control-api/openapi/selfbuild-control-api.openapi.yaml | `4a7a155d801e47590f3c60927abf6035da4fc2ca118d318f7f8204330f149cf0` | pin match, schema missing |
| tools/curated-git-apply/CONTRACT.md | `e27f397f36fe220dd4cd0f25b66499c7d36f5fca9a1bc87e13c62dd87546b9a4` | pin match, schema missing |
| tools/curated-git-apply/schema/reviewed-tree-request.schema.json | `d2b63c5f81fbcdbb47c6d27cd4128e52b1e0240e3637fabaaadd36f3e635c7ac` | read, not an HTTP operation |
| tools/curated-git-apply/schema/reviewed-tree-receipt.schema.json | `e657b1274f1003d8e6b44d271ffd8649237cc5e23674ac692fc88564add098c5` | read, not an HTTP operation |
| tools/release-control-api/openapi/release-control-api.openapi.yaml | `de281a35aa6f6a75e2a6c4cb8e1a9fb3d2f76dc20799db164c024d1bd4239946` | pin match, schema missing |
| tools/release-coordinator/src/decide.rs | `439aa2112925934c7f448f98a220fb889b708cb27fd1c7aa214b21a4d75496f4` | production_admissible false |
| tools/pg-builder/src/main.rs | `75b6366e89218e19849e4f1812f7d922512f4ea91f420a34d904347f28cb98ea` | write_json only |
| tools/agent-entry/CONTRACT.md | observed `51876a007ec288e0e8c094445e895e38bc9663e0eeaf3840e6acbf842a10063e` | untracked; pin remains `3634c0a0f580770d5cf24d29d4e4c6d56f3028acfd9ddc0fbcb5554abafd06bf` |

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL was not started. The approved PostgreSQL image pin remains `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` and was not pulled by this card.
