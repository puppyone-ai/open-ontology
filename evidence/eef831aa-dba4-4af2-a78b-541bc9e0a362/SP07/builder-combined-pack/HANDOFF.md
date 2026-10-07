# Handoff

A new independent review is queued. This session did not perform it. The a357 withhold stays on the record. `parent_acceptance` is false.

## Review the repair

Read `FREEZE.md` first. It was written before the product code and is not updated with implementation hashes.

Owned paths:

- `tools/pg-builder/**`
- `tools/description-pack/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a362/SP07/builder-combined-pack/**`

Source hashes are in `MANIFEST.sha256`. Decisive logs are `linux/proof-4/`. Earlier exits are `linux/smoke/ATTEMPT.txt`, `linux/proof/ATTEMPT.txt`, `linux/proof-2/ATTEMPT.txt`, and `linux/proof-3/ATTEMPT.txt`.

Check that `pack` and `pack_pair` still reseal one generation and still drop the other table's approved body. Check that `pack-combined` returns one product with `reseal_count` 1 and per-table provenance. Check that stale, double, conflict, and mixed-scope refuse with no pack document. Check that the `UNREVIEWED:` prefix remains when review state is unset. Check that effective column review stays null.

The stored chain used a328 `live/ready/pass1-calls/shop.{party,account,entry}-batch0.json`, `live/pass1/curated.json`, `live/s9/operator-review.json`, `live/s9/settings.json`, `live/s9/roles.json`, and `live/classifications.json`. Those files were mounted read-only. They were not copied into the image.

## Leave open

E47 acceptance, public authentication, publication, release, and full SP07 stay open. Do not treat this subset as goal acceptance. Do not grant an owner. Do not forge `review.state`.

Recipe origins, verified before delivery, are in `origin/recipes.txt`.
