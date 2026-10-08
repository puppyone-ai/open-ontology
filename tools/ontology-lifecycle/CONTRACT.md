# oo.ontology-lifecycle/v1

Owner `eef831aa-dba4-4af2-a78b-541bc9e0a403`. This file freezes the door before the Rust in this directory. `parent_acceptance` is false. `chain_closed` is false. `production_signed` is false. `compiled_pack_is_publication` is false. `certificate_class` is `not_issued`.

This crate does not write `ontology.yaml`, `reviews/decisions.yaml`, curated YAML, a Git commit, a gate file, or a role pack. It does not spawn a producer. A green test of this crate does not accept OC-01 through OC-06, SP07, or SP08.

The requirements map is `REQUIREMENTS-MAP.json`. Source head at that map is `fbd0467617a3c1514e7ffbef52f1138e20416ac6`.

## What is missing

Each marker must be a whole trimmed line in the pinned producer file. A sentence that mentions the marker is not the operation. A file whose SHA-256 differs from the pin is `unpinned` and is not executed, even if the marker is present. After an owner delivers the operation, re-read that file, record the new pin, and only then call it.

| ID | Owner | Pinned file | Marker line |
|---|---|---|---|
| OC-01 | a373 | `tools/selfbuild-control-api/openapi/selfbuild-control-api.openapi.yaml` | `operationId: exportGeneratedOos` |
| OC-02 | a373 | same OpenAPI file | `operationId: exportReviewDecisions` and `operationId: compileEffectiveYaml` |
| OC-03 | a374 | `tools/curated-git-apply/CONTRACT.md` | `operationId: applyReviewedTree` |
| OC-04 | a392 | `tools/release-control-api/openapi/release-control-api.openapi.yaml` | `operationId: evaluateGatesFromGit` |
| OC-05 | a397 | `tools/agent-entry/CONTRACT.md` | `operationId: bindActiveReleasePack` |
| OC-06 | a403 | this crate, after OC-01 through OC-05 | one private 100000-row journey |

Input, output, and acceptance for each row are in `REQUIREMENTS-MAP.json`. They are not reduced because `decideReviewItem`, `compileBuildPack`, `pg-builder approve`, curated description apply, `createRelease`, or `private_fixed` already exist.

OC-04 production external gates remain unmet. `release-coordinator` `production_admissible` returns false at `tools/release-coordinator/src/decide.rs` SHA-256 `439aa2112925934c7f448f98a220fb889b708cb27fd1c7aa214b21a4d75496f4`. This crate does not relabel that as production and does not author `gates.json`.

## Commands

```text
ontology-lifecycle inspect --source-root ROOT [--guard DIR]
ontology-lifecycle run --source-root ROOT [--guard DIR]
ontology-lifecycle serve --bind 127.0.0.1:PORT --source-root ROOT --once
```

`inspect` and `run` print one JSON object on stdout. Exit 2 means the chain is not closed. Exit 1 is usage or an unusable source root. Exit 0 would mean every gap status is `executed`. This build never emits that status.

`run --guard DIR` requires `DIR` to be an empty real directory before and after the assessment. The process does not create a file there.

`serve` listens on `127.0.0.1` only. It accepts one connection and exits.

| Request | Status |
|---|---|
| `GET /control/v1/lifecycle/assessment` | 409 assessment |
| `POST /control/v1/lifecycle/runs` with `{}` or an empty object | 409 assessment |
| POST body claiming `published`, `chain_closed`, `production_signed`, `force`, or `compiled_pack` published | 400 `malformed`, no assessment side effect |
| Any other path | 404 |
| Any other method | 405 |

The 409 body is the assessment. It includes `children_spawned: 0`.

## Dependencies

No crate dependencies. No root manifest edit. No root lock edit. `rust-version` is 1.94.0. Safe Rust only.
