# Supported matrix

Restoration note (2026-11-02): retain the dated 2026-10-01 observations below as
a historical snapshot, not a current capability assessment. The current tree
contains implementation and CI; neither presence nor this document establishes
release support or full acceptance. A new support assessment needs its own
versioned runtime evidence.

Status date: 2026-10-01. This file records what a reviewer can point at. It is not a release support matrix. No product version is supported. [SUPPORT.md](SUPPORT.md) says the same.

"Documented" means the Spec or the design docs require it. It does not mean the behavior exists.

| Capability | Identifiers | Phase | Status |
|---|---|---|---|
| Project license, NOTICE, community files, issue and PR templates, ADR template, commented CODEOWNERS | `REQ-OPS-07`, `AT-SEC-05` foundation slice | SP00-W06 | Documents are in the tree. Independent review is open. This row is not a passed full `AT-SEC-05`. |
| Dependency admission policy and blank audit-record template | `AT-SEC-05`, `R9` | SP00-W06 | Policy only. No dependency is cleared. |
| Verified dependency baseline | `REQ-CORE-01`, `AT-SEC-05` | SP00-W01 | `supply-chain/dependency-baseline.json` is present and owned by SP00-W01. This matrix does not audit it and does not clear it. |
| Cargo workspace, toolchain, crate skeletons, lockfile | `AT-CORE-01` | SP00-W01 | Skeleton crates and `Cargo.lock` are on disk. Cargo.lock is in the tree. It is not a license clearance and it is not a vulnerability scan. This matrix does not audit the lock. `crates/oo-arch-check` is a workspace dependency-direction gate, and its crate description says it is not a product service. |
| Governance document checker | `REQ-OPS-07` | SP00-W06 | `python3 governance/check_governance.py` reads these documents and does not write them unless `--write-evidence` is set. It is not `cargo audit` and it is not full `AT-SEC-05`. A skipped tool is not a pass. |
| xtask, CI gate manifest, product tests | `AT-CORE-04` | SP00-W05 | Not implemented. `.cargo/config.toml` says the reserved `cargo xtask` alias fails closed until the package exists. No CI workflow is live. |
| Rust, JavaScript, native, and image license and vulnerability scans | `AT-SEC-05` | SP00-W05 and later | Not implemented. Planned commands are labeled in [the supply-chain policy](../supply-chain/policy.md). |
| SPDX SBOM, per-artifact NOTICE, signature | `AT-SEC-05`, `REQ-OPS-03` | First M1 artifact; full offline bundle in SP14 | No artifact. |
| Synthetic data generator and public examples tree | `AT-EVAL-01`, `REQ-OPS-07` | SP00-W04 for the generator. This slice adds only the governance illustration. | `examples/README.md` and `examples/governance/synth-typed-file.md` are synthetic text. No program in this repository loads this sample. The generator is still a later task. `examples/config/` and the schema are not this slice. |
| Read-only PostgreSQL and MySQL connectors, CDC, snapshot loader | `REQ-PROD-01` | SP03–SP04 | Not implemented. |
| File import | `REQ-PROD-02` | SP05 | Not implemented. |
| Gateway, PostgreSQL grants, row-level security | `REQ-PROD-03` | SP06 | Not implemented. |
| gVisor sandbox, egress proxy, Pi | `REQ-PROD-04` | SP10 | Not implemented. |
| `./oo up`, Compose standard profile | `REQ-PROD-06`, `REQ-OPS-02` | SP11 for the M1 path, SP14 for the full matrix | Not implemented. No `oo` script and no compose file. The skeleton `oo` source says the command is not implemented. |
| MCP and OAuth | `REQ-PROD-03` | SP13 | Not implemented. |
| Formula-safe CSV and Excel file output | `REQ-SEC-02`, `AT-FILE-05`, `AT-SEC-03` | SP05-W06 | Not implemented. Formula-safe CSV and Excel output is an SP05-W06 requirement (`AT-FILE-05`, `AT-SEC-03`). No writer exists. |
| Answer HTML and Markdown rendering | `REQ-SEC-02`, `AT-SEC-03` | SP11 | Not implemented. Answer HTML and Markdown rendering is an SP11 requirement (`REQ-SEC-02`, `AT-SEC-03`). No renderer exists. No claim that an answer is safe. |
| Emergency stop and audit hash chain | `REQ-SEC-03`, `AT-SEC-06`, `AT-OPS-05` | SP11 | Not implemented. |
| Private vulnerability contact, security owner, release contact, branch protection | `REQ-SEC-03` | Before any publication | Not configured. Release blockers. |
| Customer holdout and UAT | `AT-EVAL-05`, `AT-EVAL-06` | SP12 | Not started. Public tree stays synthetic. |

Candidate versions in [docs/02 §6](../docs/02-technology-stack.md) are not rows in this matrix. They have not been locked by a build.
