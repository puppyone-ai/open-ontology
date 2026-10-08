# SP13-W01 handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a388`. Author scope is `tools/mcp-gateway/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a388/**`.

## State to record

Stdio capability for this card is implemented and the private differential exited 0 (`linux/launch.exit`, `linux/phase.txt`, `linux/marker.txt`). The protocol implementation is rmcp 3.5.0 in the child lock. Root `Cargo.toml`, root `Cargo.lock`, and `rust-toolchain.toml` were not modified.

Do not mark remote OAuth, SP13-W02, AT-MCP-02, AT-MCP-03, or the SP13 parent accepted from this packet. Do not treat `oauth=false` as an OAuth pass. Docs/08 tools without a Gateway route are not implemented here.

The development ledger stays with Codex.

## Reproduce

From the repository root, with the pinned local images already present:

`bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a388/SP13/mcp-stdio/linux/launch.sh`

The launcher refuses to start when 10 Rust builders are already running, when an `oo.owner=a388` builder or PostgreSQL is already running, or when a host `cargo` or `rustc` process is running. It creates one PostgreSQL 17.10 (1 GiB, 1 CPU) and one Rust builder (6 GiB, 1 CPU). Success removes its containers, volumes, network, and secret directory. A failing run keeps `oo-a388-cargo`, `oo-a388-target-child`, and `oo-a388-target-root`.

`linux/inside.sh` is the container script. It uses `--locked --offline` after fetch. It runs `cargo generate-lockfile` only when `tools/mcp-gateway/Cargo.lock` is absent. Do not `cargo update` and do not regenerate the lock for a rerun.

Required green files if the proof is repeated: `linux/launch.exit` = 0, `linux/marker.txt` contains `completed=1`, `linux/cargo-tree-normal.txt` has no `oo-gateway` or `pg_query`, and the root hashes match `REPORT.md`.

## Commit boundary

Commit only:

- `tools/mcp-gateway/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a388/**`

Leave every other product, frontend, root manifest, and foreign evidence path untouched.
