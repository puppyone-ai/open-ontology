# mcp-gateway contract

SP13-W01 stdio server for UUID `eef831aa-dba4-4af2-a78b-541bc9e0a388`. The frozen checklist is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a388/SP13/mcp-stdio/FROZEN-AC.md`.

The binary speaks MCP only through `rmcp` 3.5.0. It forwards three Gateway routes and returns the Gateway JSON as structured content. OAuth is not implemented. Remote filesystem export is not implemented. Those parent items are not passed by this crate.
