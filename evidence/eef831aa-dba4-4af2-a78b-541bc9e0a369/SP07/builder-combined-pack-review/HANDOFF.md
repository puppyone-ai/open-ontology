# Handoff

Verdict recorded here: **APPROVE_LIMITED** of original E47. `parent_acceptance` is false. This session did not edit product, locks, root manifests, specs, or the ledger. The E47 checkbox stays unchecked because this packet does not edit the ledger. SP07, public authentication, semantic publication, release, and parent acceptance stay open.

## Evidence

Owned path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a369/SP07/builder-combined-pack-review/**`.

- `REPORT.md` is the verdict, the proof-2 exits, the stored pack measurements, and the receipt hashes.
- `FINDINGS.md` maps that run onto the frozen E47 card and the SP07-W04 row.
- `source-inventory.sha256` is the content SHA-256 of the archived product at `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba`.
- `MANIFEST.sha256` is the content SHA-256 of this evidence directory, excluding the manifest itself.
- `CLEANUP.txt` records the removed network, volumes, and temporary archive.
- Decisive logs are `linux/proof-2/`. The failed oracle is `linux/failed-attempts/proof-1/` and was not overwritten.
- The independent oracle is `overlay/a369_review.rs`. It is host evidence. It was copied into the container only after the pre-oracle combined tests and Clippy, then deleted before the product byte compare.
- Recipe citations are `origin/recipes.txt`.

Archive commit `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba`. `tools/pg-builder` and `tools/description-pack` at HEAD `e1d77d75e1c7a4a9698cbdf6a8f5ea671dc4ca7e` match that commit. A later reader can use either tree for these two crates. The proof archived the full repair SHA.

## Leave in place

The a328 receipt tree, the a357 withhold directory, and the a362 repair directory stay as they are. Do not reload the 100000-row database and do not call grok-4.7 again for this clause while those receipt hashes still match `REPORT.md`. The per-table `pack` and `pack_pair` commands still drop the other table's approved body. That measured drop is the old command. The combined workflow output is `pack_combined`.

Do not treat this directory as SP07 acceptance, public authentication, semantic publication, a release, or parent acceptance. Do not forge `review.state`. Do not pack the two extra cache siblings listed in `REPORT.md`.
