# Architecture-check review fix

Handoff for the SP00-W01 checker corrections. This file does not update `supply-chain/dependency-baseline.md` or `dependency-baseline.json`. Those stay with the archived foundation slice.

First correction evidence: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a006/SP00/SP00-W01/`. Second correction evidence: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a010/SP00/SP00-W01/`. The archived bundle `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a001/SP00/SP00-W01/` was not modified. The review bundle `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a009/SP00/corrections-review/` was not modified.

## What the gate does now

`oo-arch-check` keeps two workspace graphs from one `cargo metadata` document.

- A required edge in `REQUIRED_EDGES` is present only when the manifest has at least one dependency that is normal, non-optional, and unconditional (`target` is absent), and whose path normalizes to that workspace member's manifest directory. A matching name with both `path` and `source` absent does not satisfy it. Dev, build, disabled optional, `cfg(...)`, and unknown kind do not satisfy it. Resolve edges, including an id that appears only in `node.dependencies`, do not satisfy it either.
- The forbidden walk still follows every manifest kind, optional flag, and target. A pathless, sourceless member name stays on that walk by name. Resolve traversal uses the union of `node.deps` and `node.dependencies`, keyed by package id. The dependency alias is not the package name. An id with no kind reports `R-DEP-KIND-UNKNOWN`. The same id on both lists is one edge. A dev-only `oo-gateway` → `oo-pipeline` edge still reports `R-GATEWAY-PIPELINE`. Unresolved path deps, dangling resolve ids, unknown members, and cycles still fail closed.

## Second correction

Review session `eef831aa-dba4-4af2-a78b-541bc9e0a009`, findings 2 and 3. Both were in-memory shapes. Live `cargo metadata` 1.98 on this workspace had zero pathless sourceless member deps and zero nodes whose `dependencies` list was non-empty while `deps` was empty.

Finding 2: a Normal, non-optional, untargeted dependency with no path and no source could satisfy `REQUIRED_EDGES` when its name matched a member. That let a name-only edge hide a dev-only path edge to `oo-connectors`. Production proof is now the normalized path. The name-only edge remains on the forbidden walk, so a name-only `oo-gateway` → `oo-pipeline` edge still reports `R-GATEWAY-PIPELINE`. Absolute member paths, relative `../` paths, a path with `.` or `..` that normalizes to the member directory, and a renamed manifest edge whose path still points at the member still satisfy a required edge.

Finding 3: `resolve_external_names` walked `node.deps` only, so a package id listed solely in `node.dependencies` was invisible. The forbidden walk, workspace-edge and cycle detection, and dangling-id check now use the union. Package identity is the id, then `Package.name`. An id-only `sqlx` on Gateway fails. The same leaf behind a benign external id fails. An id-only `oo-gateway` → `oo-pipeline` edge fails, including through `oo-sdk`. An id-only `oo-core` → `oo-gateway` edge reports a cycle. A repeated id does not emit a second copy of the same violation. Empty `dep_kinds` is an unknown kind and the package is still walked. A `deps` alias of `sqlx` whose package id is not `sqlx` is not a driver hit.

This correction does not sign SP00-W01, SP00, or the product. Independent review is still required. Full AT-CORE-01 is not done: Linux on both targets, native protocol, feature-matrix evidence, `cargo audit`, and `cargo deny` were not run. `REQUIRED_MEMBERS` is still 16. `xtask` is still absent. Gateway's `tokio-postgres`, `postgres-types`, `postgres-protocol`, and `deadpool-postgres` stay out of the Gateway deny set. Synchronous `postgres`, `sqlx`, and the driver and model families from the first correction stay denied. Policy lists were not edited.

## First correction

Session `eef831aa-dba4-4af2-a78b-541bc9e0a006`. Required edges were limited to normal, non-optional, unconditional manifest dependencies, and resolve edges stopped satisfying them. The taxonomy below was added then and was not changed in the second correction.

`REQUIRED_MEMBERS` is still the existing 16 crates. `xtask` is still absent. Adding it waits for the authorized HARNESS change that registers the member and the edges together.

`supply-chain/dependency-baseline.md` still says unknown kinds are counted as edges. That sentence now describes the forbidden walk only. Unknown kind does not satisfy a required edge. This handoff is the correction; the baseline document was left unchanged.

## Package-name taxonomy

The lists in `crates/oo-arch-check/src/policy.rs` are a reviewed taxonomy of known package names. A match does not prove what an arbitrary crate does. A name that is absent from the taxonomy is not approval to link it. Every new dependency still needs supply-chain and capability review.

Added to the driver taxonomy, on both `oo-core` / client / analysis / this gate and on `oo-gateway`:

- Diesel: `diesel`, `diesel-*`, `diesel_*` (`diesel-async`, `diesel_migrations`)
- SeaORM: `sea-orm`, `sea-orm-*`, `sea-orm_*` (`sea-orm-migration`, `sea-orm-arrow`)
- libSQL: `libsql`, `libsql-*`, `libsql_*`
- MySQL native pair: `mysqlclient-sys`, `mysqlclient-src` (alongside the existing `mysql` / `mysql_async` / `mysql_common` names)

Model SDK names now include the concrete `openai` package, plus `openai-*`, `openai_*`, `async-openai-*`, and `async-openai_*`. `async-openai`, `openai-api-rs`, `anthropic`, and `rig-core` stay covered.

Still allowed by this gate, with tests on in-memory metadata:

- Gateway read-only stack: `tokio-postgres`, `postgres-types`, `postgres-protocol`, `deadpool-postgres`
- Schema and serde names exercised here: `serde`, `serde_json`, `schemars`
- Local Arrow and file names exercised here: `arrow`, `arrow-array`, `arrow-schema`, `parquet`

`sea-orm-arrow` is classified with SeaORM. The local Arrow names above are not database credentials. Synchronous `postgres` and `sqlx` on Gateway still report `R-GATEWAY-SOURCE-DRIVER`. There is no new allowlist. Current `oo-core` dependencies (`serde`, `schemars`, `utoipa`, `jsonschema`, `sha2`, `thiserror`, `encoding_rs`, and the other names in its manifest) were already outside the deny set; this change does not add a list that would have to be edited when CORE adds another non-driver crate.

## Scope left for the coordinator

Neither correction signs SP00-W01 or full AT-CORE-01. Linux on both targets, native protocol, `cargo audit`, and `cargo deny` were not run. This slice did not run `cargo fmt --all`. Root `Cargo.toml`, `Cargo.lock`, and `crates/oo-core/Cargo.toml` were read and hashed, not edited. `oo-core` remains live CORE work. Governance findings from the same review are outside this slice.
