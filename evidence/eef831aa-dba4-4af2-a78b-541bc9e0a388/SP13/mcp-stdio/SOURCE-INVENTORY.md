# Source inventory

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a388`. Hashes of every owned file except this inventory's sibling `SHA256SUMS` are in `SHA256SUMS`. `SHA256SUMS` is produced after the other files and does not include its own hash.

## Product

| Path | Role |
| --- | --- |
| `tools/mcp-gateway/Cargo.toml` | Child package. Nested workspace. `rmcp =3.5.0`. Feature `live` enables optional `oo-gateway`. |
| `tools/mcp-gateway/Cargo.lock` | Child lock, generated once, then used with `--locked`. |
| `tools/mcp-gateway/CONTRACT.md` | Pointer to the frozen acceptance text. |
| `tools/mcp-gateway/src/main.rs` | Binary entry. `forbid(unsafe_code)`. |
| `tools/mcp-gateway/src/lib.rs` | rmcp `ServerHandler`: initialize, discover, tools, resources, prompts. |
| `tools/mcp-gateway/src/http.rs` | Loopback HTTP/1.1 client to the existing gateway. |
| `tools/mcp-gateway/tests/protocol.rs` | Real stdio child. Version negotiation and the three tools, no PostgreSQL. |
| `tools/mcp-gateway/tests/live.rs` | Ignored live differential. Feature `live`. Private PostgreSQL, gateway, compiler, and a real MCP child. |

`crates/oo-mcp` is not this card and was not edited.

## Evidence

| Path | Role |
| --- | --- |
| `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a388/SP13/mcp-stdio/FROZEN-AC.md` | Acceptance items frozen before implementation. |
| `evidence/.../CHILD-LOCK-APPROVAL.md` | Version, license, MSRV, features, checksum. |
| `evidence/.../REPORT.md` | Green-run record. |
| `evidence/.../FINDINGS.md` | License risk, earlier failures, open parent items. |
| `evidence/.../HANDOFF.md` | Reproduce and commit boundary. |
| `evidence/.../CLEANUP.md` | Owned resources removed on exit 0. |
| `evidence/.../linux/` | Launcher, container script, raw `*.exit`, stdout/stderr, phase, marker, hashes, failed-attempt copies. |

Raw exits live in `linux/*.exit`. The green set is launch, clippy-component, fetch-child, fetch-root, libclang, clippy-protocol, test-protocol, build-gateway, clippy-live, and test-live, each 0. Attempt 9 and attempt 10 clippy-live exits remain 101 as historical copies.
