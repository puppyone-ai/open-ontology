# Supply-chain policy

| | |
|---|---|
| Status | Admission contract for SP00-W06. Not a completed audit. |
| Authority | [docs/02](../docs/02-technology-stack.md) §1 and §5, [docs/10](../docs/10-security.md) §11, [docs/17](../docs/17-rust-implementation.md) §4 and §7, [Spec](../specs/open-ontology-v0.1-spec.md) §4.1 and §20, [Plan](../specs/open-ontology-v0.1-plan.md) SP00-W06 |
| Baseline file | `supply-chain/dependency-baseline.json` belongs to SP00-W01. This policy does not create or edit it. |
| Record shape | [dependency-audit-record.template.json](dependency-audit-record.template.json) |
| Scan jobs | Not implemented. Harness (SP00-W05) owns the jobs. Signing and the SBOM start with the first M1 artifact. |

A default dependency is anything linked or copied into the default build, the default container image, the default Compose profile, or the default offline bundle. An external service the operator runs themselves is not a default dependency. It is also not approved by being left out of the record.

## 1. Fields required before admission

Each default dependency needs one audit record with all of the following. A missing field means the dependency is not admissible. A comment on a ledger is not a field.

1. Ecosystem and package name.
2. Exact version.
3. Official source (registry or vendor) and the source repository.
4. Manifests that select it: `Cargo.toml` path, package manifest, image build file, or native build.
5. Publication time and adoption time, each a full RFC3339 UTC timestamp (`Z` or `+00:00`). A calendar date is not a time.
6. Elapsed time at adoption of at least `7 * 86400` seconds, or a security-fix exception that meets §4. The stored integer age is defined in §4. It is not a calendar subtraction.
7. MSRV impact, or an explicit statement that it is not a Rust crate.
8. Features that are enabled, and whether default features are turned off. Cargo default features are not a security configuration.
9. SPDX license identifier taken from the package's `LICENSE` and `NOTICE` files. The manifest's license field is a hint, not the evidence.
10. When the package is dual-licensed, the one license this project relies on.
11. Native and transitive licenses for code the build actually compiles or ships.
12. An immutable digest: crate checksum, git commit, or image digest. A floating tag is not a digest.
13. The advisory disposition for that version: a recorded scan, the scanner name, the evidence, and whether any known exploitable high-severity issue remains.
14. When the license is an expression, the license this project relies on and every obligation that choice still has.

The blank template has `"audited": false`. Leaving it that way is honest. Setting `audited` to true without the fields above is not a record this policy accepts. `default_features_disabled` is a boolean on the package. Leaving it out does not mean default features are off.

## 2. License admission

The project license is Apache-2.0. [docs/02](../docs/02-technology-stack.md) requires every dependency in the default install to be an OSI-approved permissive license, or a weak-copyleft license with a separate written review of distribution obligations.

That review names the reviewer role, the obligation (for example source offer or relinking), and where the evidence is. It is not implied by this policy.

AGPL, BSL, ELv2, and SSPL do not enter the default package. They may be a user-selected external service, audited at a specific version, and they are not copied into the default image.

The design already keeps these out of the core image: Grafana and similar observation stacks as external options; Redis 8, MinIO, Vault, and Arize Phoenix as non-default. Vault's BSL terms are noted in [docs/10](../docs/10-security.md). Adding any of them later is a new audit.

Strong copyleft that is outside the "permissive or reviewed weak copyleft" rule is also outside the default artifact. Absence from the prohibited table in §3 does not admit it.

The record syntax accepts SPDX identifiers, `AND`, `OR`, and parentheses. `WITH` and the `+` or-later form are unsupported and fail closed. They are not treated as one license id by matching the whole string.

`MIT OR Apache-2.0` can be admitted when `dual_license_choice` names one of those two licenses and the LICENSE evidence covers every license the expression declares. `MIT AND Unicode-3.0` keeps both obligations. Evidence or an obligation list that names only one side does not cover the other.

GPL-2.0 and GPL-3.0, in `-only`, `-or-later`, or the bare id, are strong copyleft. They stay out of the default artifact even though §3 does not list them, and even when an `OR` expression also names a permissive license. An unknown id, a non-OSI license, and any `LicenseRef-` id stay out. `LicenseRef-Proprietary` is not admitted.

Weak copyleft (LGPL, MPL, EPL, CDDL) needs `weak_copyleft_review` on that component: a named reviewer, the reviewer role, the obligations, an evidence path, and a scope that lists every weak-copyleft identifier in force there. The value `true` is not a review. A review of a different component, or of a different identifier, does not cover this one.

`license_evidence[].source` is an artifact locator. It is a relative or absolute path whose basename is `LICENSE`, `LICENCE`, `COPYING`, or `NOTICE`, with an optional suffix such as `.md`, `.txt`, `-MIT`, or `-Apache-2.0`, or it is an absolute `https` URL whose decoded path basename is that kind of name. A sentence, the manifest license field, a path that contains `..`, a `file://` URL, and a URL with credentials are not locators. The governance checker accepts that shape and does nothing else with it. It does not open the path, fetch the URL, or treat the shape as proof that the artifact exists. The scan in §5 still has to read the artifact and record its digest.

An obligation has to name a real duty. Empty text, and the placeholders `true`, `yes`, `no`, `n/a`, `na`, `none`, `pending`, `unknown`, `not-reviewed`, and `not applicable`, do not count, in any letter case or surrounding whitespace. The same rule applies to a weak-copyleft review and to every leaf of an `AND`. A named duty, such as retaining a copyright notice or offering source for a library, still counts. The checker is still looking at the record. It is not reading the license file.

A prohibited token from §3 blocks the default record even when it is only one side of an `OR`, and even when `prohibited_license` is false. Each `native_transitive` entry is an object with its own license evidence. A string such as `SSPL-1.0` is invalid, not ignored.

Datasets and external benchmarks keep their own license and attribution. They are not folded into the Apache-2.0 tree. Customer data is not a project dataset.

## 3. Default prohibited licenses

These identifiers are prohibited in the default artifact. Policy names are the ones used in the Spec.

| SPDX identifiers | Policy name |
|---|---|
| `AGPL-3.0-only`, `AGPL-3.0-or-later`, `AGPL-3.0` | AGPL |
| `BUSL-1.1` | BSL |
| `Elastic-2.0` | ELv2 |
| `SSPL-1.0` | SSPL |

A record that marks the dependency audited and names one of these is invalid. No exception record makes a prohibited license admissible. "User-selected external service" means the component is not in the default artifact. The same block applies when the identifier is one token inside an `OR` or `AND` expression. Setting `prohibited_license` to false does not remove the token.

## 4. Seven-day rule

A version published fewer than 7 days before adoption does not enter the default baseline.

The only exception is a security fix adopted early. The record then includes:

| Field | Required content |
|---|---|
| `kind` | `security-fix-seven-day` |
| `owner_name` | A person this project has actually appointed. A role label, `OWNER_PENDING`, or a name that is not appointed does not count. |
| `owner_role` | The appointed person's role. A role with nobody appointed is not an owner. |
| `reason` | Why this version, and why the previous admissible version is insufficient. |
| `regression_evidence` | An object with a command, an integer exit code, and an artifact path. A bare string is not that evidence. |
| `waive` | Empty. |

`publication_date` and `adoption_date` are full RFC3339 timestamps in UTC, written with `Z` or `+00:00`. Fractional seconds are allowed. A date-only value, a naive clock reading, and an offset other than zero are invalid. A calendar date cannot prove that `7 * 86400` seconds elapsed. The checker rejects that value. It does not truncate it to midnight and it does not invent a time of day.

`age_days_at_adoption` on this audit record is an integer: how many 24-hour intervals were completed, `floor(elapsed_seconds / 86400)`. Package admission does not use that integer as the seven-day test. It compares the exact elapsed duration, fractional seconds included, with `7 * 86400` seconds. An elapsed duration of 604800 seconds or greater can pass when the stored integer is that floor. A shorter duration fails, including 604799.999999 seconds, 518401 seconds, and a publication one second inside a cutoff. Adoption before publication fails. A boolean age, a non-integer, NaN, or an infinity fails. A future instant is compared with an explicit UTC `as_of` datetime and with actual UTC now, not with a calendar day. A caller-supplied date at midnight is not permission to treat a later instant that day as already past.

`supply-chain/dependency-baseline.json` is a different document. Rows there may store a fractional `age_days_at_adoption`, for example `7.142`. That float is historic baseline metadata. It is not the canonical age of an audit record governed by this section, and copying the float into the audit record does not satisfy it. A baseline publication timestamp can be represented on an audit record when it is a real RFC3339 UTC instant and the stored age is recomputed as the floor above. The baseline `thiserror` 2.0.21 row was published at `2026-09-23T20:24:07.771104Z` and adopted at `2026-09-30T23:48:21Z`. That elapsed time is greater than `7 * 86400` seconds, and the floor is 7, not `7.142`. This policy does not edit the baseline.

Nobody is appointed while `.github/CODEOWNERS` still says `OWNER_PENDING` and SECURITY.md still says the security owner is not appointed. Until that changes, a seven-day security exception is not valid.

`waive` stays empty because this exception does not waive:

- a prohibited license
- a known exploitable high-severity vulnerability
- an authorization bypass
- a credential or secret-isolation failure
- a sandbox or network isolation failure
- a source-write defect

Those stop the default artifact. The response is to upgrade, replace, or remove the component, or to stop the affected capability. [Spec §20.2](../specs/open-ontology-v0.1-spec.md): a supply-chain exception cannot exempt a known exploitable authorization, credential, or isolation defect.

The Plan's rule for the first quality baseline (a frozen bootstrap, then a seven-day window) is a separate rule about evaluation scores. It does not let a young dependency into the default build.

## 4.1 Advisory disposition

An audited record carries an advisory block. `scan_recorded` is true. `scanner` names the tool the reviewer ran. `evidence` points at the output that was kept. `disposition` is `no-known-advisories` or `resolved-before-adoption`. `known_exploitable_high_severity` is false.

Absence, a null, or a pending disposition does not clear the record. A known exploitable high-severity issue cannot be waived by this block, by `waive`, or by a ledger note.

The governance checker validates that shape. It does not run `cargo audit`, `cargo deny`, or an image or native scanner. Those scans are §6. A record that says they ran is not a substitute for running them later, and a missing scanner is not a pass.

## 5. Scan scope

When scan jobs exist, they cover every row below. A check of the top-level Cargo license field does not meet this policy.

| Surface | What is included |
|---|---|
| Rust | `Cargo.lock`, license and ban policy, RustSec advisories, duplicate Arrow and TLS versions in the resolved tree |
| JavaScript | The lockfile for `console/` and `sdk-ts/`, once those trees exist |
| Native | `build.rs`, proc-macro crates, C and C++ (libpg_query, embedded PostgreSQL, DuckDB, bindgen), vendored sources, their `LICENSE` and `NOTICE` |
| Images | Base image by digest, OS packages, non-root runtime user |
| Other | OSV across those ecosystems; offline extensions and fonts shipped in the runtime image |
| Secrets | The artifact that will be distributed: no production master key, private key, source password, token, or customer data |

`AT-SEC-05` in full also covers tampering with an offline signature and a secret scan of the bundle. Those need an artifact. This policy does not claim they have been run.

`python3 governance/check_governance.py` checks governance documents and the shape of an audit record. It is not one of the scans above. Its default run writes nothing. A skipped yamllint, or a `git diff --check` that does not see untracked files, is not a passing scan. The checker does not fetch a license URL and does not read an upstream LICENSE. A locator that is well formed is not a scan result.

## 6. Planned commands

These commands are specified by [Spec §18.3](../specs/open-ontology-v0.1-spec.md) and [docs/17 §7](../docs/17-rust-implementation.md). SP00-W06 does not implement them. Harness owns the jobs. A machine that does not have the tool is not a passing scan.

```bash
# Planned — harness and later release jobs. Not a SP00-W06 result.
cargo audit
cargo deny check
cargo tree -d
```

JavaScript, image, and OS-package scans are in scope and do not have a command name in this repository yet. The job that adds them names the command in the same change. This policy does not invent a green result for them.

## 7. Digests and untrusted builds

- Actions, images, builders, and tools are pinned to a commit or a digest a reviewer can open.
- Cache keys may include the lockfile, toolchain, features, and platform. They do not include customer data or secret paths.
- A tag promotes an already verified digest. It does not rebuild a new artifact and call it the same release.
- `build.rs` and proc-macros execute at build time. A new dependency or a fork pull request makes that execution untrusted.
- Those builds run without production secrets, customer secrets, or production OIDC rights.
- Fork pull requests do not receive release secrets or customer secrets.
- A `pull_request_target` workflow does not also execute pull-request code with elevated permissions.
- Containers from untrusted pull requests do not run on production hosts or on long-lived sensitive self-hosted runners.
- Hook and plugin artifacts are built in that isolated way, then signed, with an SBOM and a manifest, before deploy. Production start does not run `cargo install` or compile the config repository.

## 8. SBOM, NOTICE, and signing

Required on the first M1 artifact. Not deferred to SP14. SP14 adds the offline bundle and the rest of the release matrix.

That artifact ships:

- an SPDX SBOM
- a NOTICE that lists the third-party notices verified for that digest
- a signature over the digest that will be distributed
- a record of the build source

The repository [NOTICE](../NOTICE) licenses first-party work. It does not clear unverified dependencies. SP00-W06 has no artifact, no SBOM, and no signature. Privacy scanning of a runtime artifact is later work (`AT-SEC-05` full scope and `AT-OPS-03`).

## 9. Adding a record

1. Open a dependency issue with the form under `.github/ISSUE_TEMPLATE/dependency.yml`, or include the same fields in a pull request.
2. Fill a record from the template. Do not set `audited` to true without the evidence fields.
3. Do not edit `supply-chain/dependency-baseline.json` in a governance-only change. The foundation owner updates the baseline after review.
4. Native code, `unsafe`, new dependencies, and hooks need the Rust reviewer and the security / supply-chain reviewers ([Spec §19](../specs/open-ontology-v0.1-spec.md)).
5. A prohibited license or a known exploitable high-severity issue blocks admission into the default set.

## 10. What this document is not

- It is not `dependency-baseline.json`.
- It is not a scanner configuration.
- It is not evidence that a scan ran.
- It is not permission to download customer data in order to test a dependency.
