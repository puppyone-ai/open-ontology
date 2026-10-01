# Evidence v1

`contracts/evidence/evidence-v1.json` is the canonical schema, `$id` `https://openontology.dev/contracts/evidence/v1`, draft 2020-12, `additionalProperties` false. `xtask` embeds that file. The checker does not rewrite historical manifests.

`cargo xtask evidence --check --path MANIFEST` validates the document and the files it names. Exit 0 means the document is honest and valid, including an honest `in_review`. It does not appoint a reviewer and it does not close the phase.

## What a project `passed` requires

The checker applies its own budgets before it calls the JSON Schema library: 1 MiB, depth 32, 50000 nodes, no duplicate keys, and every number must be an exact `i64` or `u64`. A huge exponent, a fractional exit, a deep tree, or a duplicate key is an error. The checker does not catch a panic and call the document passed.

- `commit` is a real git object whose `git show COMMIT:path` bytes match every source hash, or an honest `uncommitted` together with matching source hashes while status is not `passed`. `passed` plus `uncommitted` fails.
- `source_hashes` covers every scope input. A directory input expands to the regular files under it. An empty hash list fails. `kind: fixture` with `full_case: true` fails.
- `version_tuple` fields that apply are non-empty on a passed document. Schema, harness, and suite are required when present.
- Every acceptance row on a passed non-fixture document is `passed`, exit 0, and `full_case` true.
- A report or log artifact needs `artifact.binding` carrying commit, suite, `schema_version` `evidence/v1`, and `content_sha256`. A plaintext log does not need to contain JSON. An unbound report or a binding for a different build fails `evidence.replay_wrong_build`.
- `signed_aggregate` is false until a real signature artifact exists. `signed_aggregate: true` fails with `evidence.signature_deferred`.
- Artifact paths stay inside the evidence directory. Symlinks, `..`, absolute paths, and `~` are rejected. Bytes and size are recomputed.
- Reviewers are independent appointed logins. The author is not the reviewer. A synthetic name is not a role. Fixture scope may use a `fixture-` reviewer with context `Fixture` and `project_acceptance` false.
- The security owner is unappointed while `SECURITY.md` says so and CODEOWNERS has no uncommented login. A security waiver then fails. A public author cannot forge a private or customer aggregate. Those paths are not opened.

Customer, gold, and holdout paths are not opened. The error text does not copy those markers.

A runner capture directory is an integrity record of bytes. It is not an evidence pass and it is not execution authority. `gates --check` can recompute the public binding and still leave the import unverified. Local engineering success is the in-process allowlisted spawn (`gates --execute-local`). That private result is not a field a manifest can deserialize. Release authority, CI, and a signed aggregate stay absent until a real external attestation exists. This checker does not invent one.

## Spec example

The example in the specification is illustrative. It is not a valid v1 document. v1 adds the scope, status, version tuple, source hashes, command interval, exit, artifact size, environment privacy, and reviewer fields that the example omits. Historical a001–a022 manifests use the older draft shape. See `historical-draft.md`.

## Emit

```text
cargo xtask evidence --emit --dir DIR --write
cargo xtask evidence --check --path DIR/manifest.json
```

`--write` recomputes artifact hashes and source hashes. It refuses status `passed` and refuses to open a private path. The manifest should not list itself.
