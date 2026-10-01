# Coordinator decision: SQLx 0.9 explicit source configuration and CA trust

Status: frozen implementation direction, conditional on a066 confirming the public API limitation. Implementation and acceptance remain unproven. No customer or release approval is implied. Applies only to the source correction; SQLx remains the prescribed source/control driver.

## Evidence

a061 fixed-source review reproduced ambient PGOPTIONS changing current_user and admitted a write-capable login. Source commit b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74 selected sqlx-core/sqlx-postgres 0.9.0. Coordinator inspected their registry sources in a061's isolated cargo-home after cleanup:

- sqlx-postgres/src/options/mod.rs: new_without_pgpass reads PGPORT/HOSTADDR/HOST/USER/DATABASE/PASSWORD/SSLROOTCERT/SSLCERT/SSLKEY/SSLMODE/APPNAME/OPTIONS; skipping pgpass does not remove environment fallback. Relevant fields are private; options() appends rather than constructing a blank field set.
- sqlx-core/src/net/tls/tls_rustls.rs: ring provider cfg requires ring-webpki or ring-native-roots; the normal verified handshake starts with import_root_certs and then adds caller CA. The empty-root import function exists, but no supported ring-no-roots feature selects it.

These are code-path observations, not proof that a patched build passes. Registry origin/checksums/version/license bodies were independently recorded by a061. The correction must preserve that provenance and produce its own evidence.

## Approved minimal correction if public APIs cannot satisfy the requirements

1. Vendor only the two required official 0.9.0 crates, sqlx-postgres and sqlx-core, in the existing product checkout's crates/oo-connectors/vendor. This matches the executor's actual owned directory; no additional vendor tree is needed. Use verified crates.io archives as upstream bases, preserve licenses/copyright and normalized manifests. Record archive checksums, upstream tree inventory, exact patch diff, resulting tree hash and patch identity. Do not vendor the whole registry or silently change dependency versions.
2. Add a documented public PgConnectOptions constructor that reads no environment, pgpass, user/home or certificate files. It sets only deterministic inert defaults; oo-connectors supplies all validated required connection fields, verify-full, caller CA, credentials and budgets explicitly. No global environment mutation, unsafe field access, serialization tricks or ambient client certificate/key. Existing SQLx constructors remain unchanged for callers that deliberately use upstream defaults.
3. In the Rustls verified handshake, an explicitly configured root CA starts from an empty RootCertStore and loads only that configured CA bundle. Absence of a caller CA may retain the upstream import_root_certs behavior; oo-connectors must require a caller CA and cannot invoke that fallback. Preserve certificate validity/hostname checks and ring provider; never enable accept_invalid_certs/hostnames. No need to alter the TlsConfig struct or add broad TLS plumbing if this bounded change suffices.
4. Bind the two exact patched crates using explicit Cargo patch paths in the existing checkout and record selected versions/source paths in the component manifest. Clearly identify the result as an Open Ontology patched component, not byte-identical upstream 0.9.0. A matching version string alone is insufficient provenance. Future SBOM/release attestations must distinguish the fork and reference original upstream packages plus patch digest.
5. Add upstream-level and product regressions for no-env construction and exclusive caller CA trust. Test hostile ambient PGOPTIONS/SSLCERT/SSLKEY in isolated subprocesses without mutating process-global environment; compare actual session_user/current_user and registration outcome. Prove the effective caller-CA store composition, wrong CA/hostname failure, default-root fallback unreachable from source API, and normal safe-reader success. Retain all global privilege regressions from a061.

## Boundaries and validation

This exception authorizes the smallest auditable upstream patch when supported APIs are unavailable. It does not authorize changing to another driver, a large fork, disabling TLS, relying on system/public roots for source admission, source DDL/writes, loosening role checks or modifying other components. If the patch requires materially broader changes, report the exact blocker and diff proposal before broadening.

Grok remains the sole implementation/test executor. Codex freezes scope and checks final evidence. No new worktree: use the existing PG checkout only, then integrate source-specific changes into main after its shared writer finishes. Main root lock remains owned by a063. No main source edits or integration from this patch task.

Run focused Linux/MSRV smoke before new patch implementation and final locked source/upstream regression tests plus Clippy on Linux and host. Preserve real failed commands. Audit actual graph/checksums/age/MSRV/license bodies and exact selected features. Native/public CA data can remain compiled with the ring-webpki feature; its CDLA-Permissive-2.0 distribution obligation is not erased because source clients use an exclusive root store. a067 currently adds only ISC; any remaining CDLA policy/NOTICE correction is a separate explicit foundation task.

The correction commit must contain the minimal source patch, manifests/provenance/licenses, product regressions, accurate TlsReport and handoff. Independent review targets the exact new commit and must assess the fork, env/CA regressions and actual source audit. Until that review and applicable integration checks pass, registration and import remain unaccepted. Full SP03/P14/vertical flow and release gates remain open.
