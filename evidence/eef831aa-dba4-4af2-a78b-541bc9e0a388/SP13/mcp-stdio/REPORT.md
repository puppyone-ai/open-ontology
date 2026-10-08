# SP13-W01 stdio report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a388`. This report records the stdio card. It does not sign parent SP13, AT-MCP-01, AT-MCP-02, AT-MCP-03, or remote OAuth. The development ledger is unchanged.

## Result

The owned launcher exited 0. Phase file `linux/phase.txt` ends at `09:12:00 done`. Marker `linux/marker.txt` is `ran=1` then `completed=1`.

| Check | Exit | Evidence |
| --- | --- | --- |
| Rust 1.94.0 arm64 smoke, before product edits | recorded `SMOKE_OK` | `linux/smoke-rustc.txt` |
| Clippy component for toolchain 1.94.0 | 0 | `linux/clippy-component.exit` |
| Child fetch `--locked` | 0 | `linux/fetch-child.exit` |
| Root fetch `--locked` | 0 | `linux/fetch-root.exit` |
| Production `cargo tree --edges normal` | clean | `linux/cargo-tree-normal.txt` |
| Protocol Clippy `-D warnings` | 0 | `linux/clippy-protocol.exit` |
| Protocol tests | 0 | `linux/test-protocol.exit`, `linux/test-protocol.stdout` |
| `oo-gateway` and `role-pack-bundle` build | 0 | `linux/build-gateway.exit` |
| Live Clippy `--features live -D warnings` | 0 | `linux/clippy-live.exit` |
| Ignored live differential | 0 | `linux/test-live.exit`, `linux/test-live.stdout` |
| Launcher | 0 | `linux/launch.exit` |

Protocol stdout: 3 library tests passed, and 3 child-process tests passed (`discover_target_version_lists_the_same_tools`, `initialize_previous_version_lists_gateway_tools_only`, `initialize_rejects_unknown_and_non_handshake_versions`). Live stdout: `stdio_matches_live_gateway_for_both_protocol_versions` passed. The live test is `#[ignore]` and ran with `--ignored`. `OO_A388_LIVE=1` is required; the marker is written only by that test.

## What the live process proved

One private PostgreSQL 17.10 (`linux/roles.txt` ends `roles_ready`) and one arm64 Rust 1.94 builder. Images were the pinned digests in `linux/pg-image.txt` and `linux/rust-image.txt`, both `arch=arm64`, pull never. The gateway binary and `role-pack-bundle` were compiled in that builder. The public compiler fixtures produced picker and auditor packs. The hidden visibility fixture was refused with `classification.unknown_pii`. A real `mcp-gateway` child spoke stdio through rmcp `TokioChildProcess`.

The ignored test checks both lifecycles against independently parsed Gateway HTTP:

- Initialize accepts `2025-11-25`. Discover accepts `2026-07-28`. Initialize of `2026-07-28`, `2025-06-18`, and `1999-01-01` returns JSON-RPC `-32022` (protocol test).
- Tools are `session_current`, `pack_metadata`, and `query`. `structuredContent` equals the HTTP JSON. `query_id` and `warnings` are absent. Non-2xx bodies stay `isError` with that JSON.
- Picker columns are `order_id`, `picker_note`. Auditor columns are `auditor_phone`, `order_id`. The digests differ. Swapping the token file in one process changes the role.
- `resources/read` of `oo://knowledge/{path}` returns the pack file object (`path`, `bytes`, `sha256`) as `application/json`, with `ttlMs` 0 and cache scope private. The object has no prose `text` field.
- SQL `POST /v1/sql` is the Gateway `SQL_UNSUPPORTED` 501 envelope. The statement is not echoed.
- A session token is the Gateway `UNAUTHENTICATED` 401 envelope.
- `export` is `METHOD_NOT_FOUND` and does not read or write the canary file. `file:///tmp/oo-a388-export` is `INVALID_PARAMS`.
- stderr carries the startup line `oo-mcp-gateway: stdio ready oauth=false remote_export=disabled` and method/status lines. Token bytes, the runtime password, the SQL marker, and the canary are absent from the logged process surfaces.

Startup line `oauth=false` records that OAuth is not implemented. It is not an OAuth pass.

## Dependency binding

`rmcp` is exactly 3.5.0. Child lock checksum `fae7019994ae0fe4ada40b732f798f3ff26f0f04facb1477f1bf37eb4f18a2d3` (`linux/rmcp-checksum.txt` and `tools/mcp-gateway/Cargo.lock`). Features: `server`, `client`, `transport-io`, `transport-child-process`. Default features are off. The successful run found the child lock already present and did not run `cargo generate-lockfile` again (`linux/lock-generated.txt`).

Production tree contains `rmcp v3.5.0` and does not contain `oo-gateway` or `pg_query`. `oo-gateway` is an optional normal dependency behind feature `live`, used only by the ignored test.

## Root manifests

Host hashes before and after the launcher match, and the in-container hashes match:

- `Cargo.lock` `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`
- `Cargo.toml` `b772f8fb4873885fcfac747eb24163559031bb9192ba99e8a8472571c75822bc`
- `rust-toolchain.toml` `7553618f800829444096beea3dce84c3d07585efda504c19adb6787369bbd9e8`

`git status` of those three files was empty at collection. Proof HEAD at collection was `694c799a2eb33b8936ae433180b735a95db9380a` for the rest of the tree. This card does not edit them.

## License and MSRV

Public record, also in `CHILD-LOCK-APPROVAL.md`:

- Crate `rmcp` `=3.5.0`, tag `rmcp-v3.5.0`, annotated tag object `d01d84bf1623ffb4f23dbf772fe4fc2735a46a0e`, published `2026-09-28T00:16:12Z`.
- Manifest license Apache-2.0. LICENSE blob `4a93985763241755401a10678395303de4e720ba`: new code Apache-2.0; contributions that were not relicensed remain MIT; CC-BY-4.0 covers non-specification documentation, which this binary does not ship.
- Upstream MSRV is workspace `rust-version` 1.88. The proof compiler is rustc 1.94.0 (`4a4ef493e3a1488c6e321570238084b38948f6db`) on `aarch64-unknown-linux-gnu`.
- No `cargo-audit` run and no advisory disposition is claimed. `supply-chain/` was not edited.

## Recipes

Mounted read-only and hashed in `linux/recipe-hashes.txt`:

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg/pg-wrap.sh` `238d2d9828dab0e2f8402cc935adf324890b4802aeba842679bd3473e49ebbf6`
- same directory `init-roles.sh` `a84c4fda4ae913e894f460f6fbaabf767c7ba174e738b0701be05c5379491f94`
- `select-debs.rs` `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644`
- `sha256.rs` `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a345/SP06/gateway-compiled-pack-review/linux/smoke-3/extract-libclang.sh` `33a79ac02e2aa3ca4cab4965f6b0cf90703491b55cc3938092090468637fa277`

Clippy install followed the reviewed already-installed check, then `rustup component add clippy --toolchain 1.94.0` when the fresh container image lacked the component. That command exited 0.

## Still open

Remote Streamable HTTP, OAuth, RFC 9728, PKCE, and token exchange stay on SP13-W02 / AT-MCP-02 / AT-MCP-03. Docs/08 tools with no live Gateway route stay out of this binary. Parent and goal acceptance stay open for Codex and the ledger.
