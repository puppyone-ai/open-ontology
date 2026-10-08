# Child lock approval — rmcp 3.5.0

Root `Cargo.toml` and root `Cargo.lock` do not contain `rmcp`. This card does not add them. The production dependency is a new child manifest at `tools/mcp-gateway/Cargo.toml` with its own `Cargo.lock`, generated once by `cargo generate-lockfile` inside that directory. Later commands use `--locked`. This is not `cargo update` and not a root manifest edit.

| Field | Value |
| --- | --- |
| Ecosystem | crates.io / GitHub `modelcontextprotocol/rust-sdk` |
| Crate | `rmcp` |
| Version | `=3.5.0` |
| Tag | `rmcp-v3.5.0` |
| Annotated tag object | `d01d84bf1623ffb4f23dbf772fe4fc2735a46a0e` |
| Release published | `2026-09-28T00:16:12Z` |
| Adoption | child lock kept after one `cargo generate-lockfile`. The successful run recorded `linux/lock-generated.txt` as already present and did not generate again. |
| Elapsed | publication to 2026-10-08 is more than `7 * 86400` seconds |
| MSRV | workspace `rust-version` `1.88` at that tag. Project toolchain for the proof is `1.94.0`. |
| Manifest license | `Apache-2.0` |
| LICENSE blob at tag | `4a93985763241755401a10678395303de4e720ba` |
| LICENSE remainder | new code is Apache-2.0. Contributions not relicensed remain MIT. CC-BY-4.0 applies to non-specification documentation, which this card does not ship. |
| Enabled features | `server`, `client`, `transport-io`, `transport-child-process` |
| Default features | off. That leaves out `base64`, `macros`, `auth`, streamable HTTP, and `request-state` (the feature that pulls `sha2` 0.11). |
| Schema | `rmcp::model::ProtocolVersion` constants `2025-11-25` and `2026-07-28` in crate 3.5.0. Product support is those two versions only. |
| Direct pins shared with the root lock | `serde =1.0.229`, `serde_json =1.0.151` with `arbitrary_precision`, `tokio =1.53.1`, `zeroize =1.9.0` |
| Registry checksum | `fae7019994ae0fe4ada40b732f798f3ff26f0f04facb1477f1bf37eb4f18a2d3` in `linux/rmcp-checksum.txt` |
| Advisory scan | not run. This record does not claim `cargo-audit` or a clean advisory disposition. |
| `supply-chain/` | not edited |

`oo-gateway` is an optional normal dependency. Cargo 1.94 rejects an optional dev-dependency. The `live` feature is the only switch that enables it, and the production build does not pass that feature, so the production binary does not link `oo-gateway` or `pg_query`.
