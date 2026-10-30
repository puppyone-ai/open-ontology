# Product Source Synchronization

`open-ontology` is the reusable product repository. Customer delivery
repositories are integration and evidence workspaces, not source mirrors.

## Allowed To Sync

- Rust crates and product libraries.
- Source connector, CDC, reconciliation, Loader, and publication code.
- Generic Self-build, review, Pack, Gateway, CLI, SDK, and MCP code.
- Portable contracts, architecture docs, operations docs, and neutral tests.
- Synthetic fixtures without customer names, credentials, endpoints, raw rows,
  model keys, or private filesystem paths.

## Must Stay Outside

- Customer business Skills and customer-specific prompts.
- Gateway tokens, model bearer files, SSH keys, EIP addresses, database
  passwords, VPN material, and private certificates.
- Customer schema/catalog exports, raw rows, business reports, and query
  traces.
- Runtime state, launch-agent files, scheduler state, publication receipts,
  model traces, and acceptance evidence.
- Customer handoffs, price quotes, and incident reports.

## Merge Procedure

1. Compare product worktrees by tracked path; never bulk-copy a delivery
   repository.
2. Use the product source as the incoming version while preserving unrelated
   local destination changes.
3. Resolve shared migration and scheduler conflicts explicitly. Keep migration
   order, immutable checksums, and tests coherent.
4. Scan paths and text for credentials, private endpoints, customer names,
   raw data, and absolute private paths.
5. Run `cargo check --offline --locked --workspace` and focused tests.
6. Review `git diff` and the private-scan report before staging.

This procedure does not imply that a customer deployment, CDC source, Gateway,
or business-answer acceptance is complete. Those remain runtime gates tracked
in delivery evidence.
