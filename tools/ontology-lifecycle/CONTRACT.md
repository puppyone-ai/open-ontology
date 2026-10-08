# oo.ontology-lifecycle/v1

Owner `eef831aa-dba4-4af2-a78b-541bc9e0a403`. This file freezes the door before the Rust in this directory. `parent_acceptance` is false. `chain_closed` is false. `production_signed` is false. `compiled_pack_is_publication` is false. `certificate_class` is `not_issued`.

This crate does not write `ontology.yaml`, `reviews/decisions.yaml`, curated YAML, a Git commit, a gate file, or a role pack. It does not spawn a producer. A green test of this crate does not accept OC-01 through OC-06, SP07, or SP08.

The requirements map is `REQUIREMENTS-MAP.json`. Source head at this read is `9c084b58d65f63229355fe6c20fd2f0285d6f29a`. The diff from `5bc2350822db765610e3fdc6e4646fd2ec27c191` is evidence under SP12 only. The eight OC document hashes are unchanged.

## What is missing

Readiness is the success schema, not an operation name. Any operation id is eligible when its 200 or 202 schema requires the fields in the map and does not carry a disqualifying const such as `compiled_pack: not_published`, `ledger_authenticated: false`, or `authenticated: false`. A contract without OpenAPI can declare the same fields with `method:`, `path:`, and `required_response:`. Those three lines are read per heading, so an older section that still says `authenticated: false` does not hide a later section and does not itself match. Equivalent response names are `new_commit` for `commit`, `new_tree` for `tree`, `old_commit` for `expected_head`, and `blobs` for `curated_blob`. A line that only names an operation is not ready. A file whose SHA-256 differs from the pin is `unpinned` and is not called. After an owner delivers the schema on a fixed commit, re-read that file, record the new pin, and only then call it.

OC-03 was re-read at `tools/curated-git-apply/CONTRACT.md` SHA-256 `e27f397f36fe220dd4cd0f25b66499c7d36f5fca9a1bc87e13c62dd87546b9a4` (commit `d04da89ff696b45a4938dd59523821b5268b055f`). Request schema `d2b63c5f81fbcdbb47c6d27cd4128e52b1e0240e3637fabaaadd36f3e635c7ac`. Receipt schema `e657b1274f1003d8e6b44d271ffd8649237cc5e23674ac692fc88564add098c5`. The reviewed-tree operation is CLI `apply-reviewed-tree`. The receipt requires `authenticated` const true, `new_commit` (40 hex or null), `new_tree`, `old_commit`, `old_tree`, `ontology_blob`, and `decisions_blob`. `expected_head` is request-only. Curated ids are `blobs[]`, not `curated_blob`. `new_commit` null means outcome unchanged and is not a commit. The file has no `method:`, `path:`, or `required_response:` line. This crate does not spawn the binary or git. Dirty `src/git.rs`, `src/lib.rs`, and untracked `src/reviewed.rs` are not the contract. The edge stays `blocked_contract` / `schema_missing`.

OC-05 worktree `tools/agent-entry/CONTRACT.md` was re-read again at SHA-256 `51876a007ec288e0e8c094445e895e38bc9663e0eeaf3840e6acbf842a10063e`. An earlier read in this same phase was `1484189f995e8404d3c2fc381c258418a31e2c4a819135f7ba227b61be001aba`. The gap text did not change. That tree is untracked, so the pin stays `3634c0a0f580770d5cf24d29d4e4c6d56f3028acfd9ddc0fbcb5554abafd06bf` and the edge stays `unpinned`. The line `operationId: bindActiveReleasePack` is not a success schema. `POST /v1/query` remains `private_fixed`. The 201 session text names `pack_sha256` and does not require `active_release_id`, `source_commit`, `policy_sha256`, `knowledge_sha256`, and `content_sha256` together. It is not called.

The client sends `__Host-oo-session`, `X-OO-CSRF`, and `Origin` to `127.0.0.1` after `GET /control/v1/identity/current` returns that same CSRF and `authenticated: true`. It records the request and response SHA-256. A later run with the same journal reads the operation back and does not POST again. `inflight` means the POST started and the response was not stored; the next run does not repeat it. `cleanup` removes only regular files listed in `owned.txt` under the state directory.

`table_row_counts` are per table. The approved fixture is `shop.party` 2000, `shop.account` 8000, and `shop.entry` 90000. A response that puts 100000 on a file is refused. OC-06 stays blocked until the five edges link by hash and the failure loops have no review, Git, or publish side effects. This build does not set `chain_closed`.

Input, output, and acceptance for each row are in `REQUIREMENTS-MAP.json`. They are not reduced because `decideReviewItem`, `compileBuildPack`, `pg-builder approve`, curated description apply, `createRelease`, or `private_fixed` already exist.

OC-04 production external gates remain unmet. `release-coordinator` `production_admissible` returns false at `tools/release-coordinator/src/decide.rs` SHA-256 `439aa2112925934c7f448f98a220fb889b708cb27fd1c7aa214b21a4d75496f4`. This crate does not relabel that as production and does not author `gates.json`.

## Commands

```text
ontology-lifecycle inspect --source-root ROOT [--guard DIR]
ontology-lifecycle run --source-root ROOT [--guard DIR] [--state DIR] [--session FILE] [--identity URL] [--producer OC-01=http://127.0.0.1:PORT]
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
