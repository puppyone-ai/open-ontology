# Historical evidence drafts

Manifests a001 through a022 were written before evidence v1. They stay in their original files. `evidence --check` does not rewrite them and does not treat them as canonical v1.

## Recognition

A file is a historical draft when all of these hold:

- `evidence_schema_version` is absent
- `spec_id` is `OO-SPEC-001`
- and one of `worktree_note`, `plan_id`, or an artifact object that uses `bytes` without `size`

## Adapter

Without `--adapter`, the canonical check exits 1 with code `evidence.historical_draft`.

```text
cargo xtask evidence --check --path MANIFEST --adapter historical-draft
```

That command exits 3. The report status is `historical_draft`, `canonical_v1` is false, `rewritten` is false, and `opened_escaped_paths` is false. The checker does not follow a path that leaves the evidence root.

Any other adapter name exits 2.

A current manifest written by `evidence --emit` is v1. Passing it through the historical adapter is not a substitute for `--check` without an adapter.
