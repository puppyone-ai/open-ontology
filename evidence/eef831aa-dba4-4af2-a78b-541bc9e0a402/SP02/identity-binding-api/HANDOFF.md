# Handoff

Session: `eef831aa-dba4-4af2-a78b-541bc9e0a402`.
Owned paths: `tools/control-identity/**`, `tools/identity-binding-api/**`, `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a402/SP02/identity-binding-api/**`.
Bind this commit for G1/G2 on SP02-W05/B01 only.

The extension stays on contract `oo.control-identity/v1`. The five a6f92 operations are unchanged on the wire. Preview and approve are additive. The consumer crate speaks HTTP only, so a fixed archive of control-identity is enough for it. a400 was not edited.

`a402-proof.json` is the closed loop on private PostgreSQL 17.10 and the signed test IdP: admin, reviewer, auditor, and business_owner. `a370-proof.json` is the existing identity suite on the same cluster, run first, while the schema was empty.

## Left open

Full SP02 stays open. A customer IdP was not used. Release acceptance was not claimed. `authorize()` still evaluates the operator file, as F3 describes. A database that already has only the v1 manifest is drift. Data-role GRANTs in `roles.yaml` are out of scope. The Git document is management bindings only. No network push and no host repository index.

## Evidence

- `REPORT.md`, `FINDINGS.md`, `FAILED-ATTEMPTS.md`, `RAW-EXITS.md`, this file.
- `smoke/` is the pre-edit Rust 1.94 lib and cli run. Both exits 0.
- `verify-console.log` is the final clippy plus a370 plus binding loop. All three exits 0.
- `final-console.log` is the cli test and the binding library tests. Both exits 0.
- `a370-proof.json` and `a402-proof.json` have no password and no PEM.
- `source-inventory.txt` lists the owned product files and their SHA-256.
- `SHA256SUMS` lists this evidence directory. `SHA256SUMS.sha256` is the hash of that list.
- `cleanup/run.log` records removal of the a402 container, network, and volumes.

One commit, owned paths only, no push.
