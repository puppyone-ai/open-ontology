# L5 Suggest evidence (B-S8-02)

Run `be2-l5-3` of `tools/oo-test/super_ontology_l5.py` (real `oo-docs-engine` under real `oo serve`, labelled PostgreSQL
`postgres@sha256:839fbb5a…`, real `oo context mount|collect`, `oo docs suggestion`), 34 checks, 0 failed. Needs migration 0143
(coordinator patch `B-L5-migration-docs-suggest.patch`) and the docs-api 1.3 contract patch. No token or credential is in
these files.

| Acceptance (30 §L5) | Result | Numbers | Files |
|---|---|---|---|
| 1 the six steps of section 4 of the idea document, accept at most 2 s | PASS | accept 0.021 s (API), 0.026 s (CLI, process start included); the old Share's `UPDATES.md` has the broadcast entry, `oo context check` exits 10; the next Share's BRIEF has 小王 as owner and 跟进中 | `a1-BRIEF-before.md`, `a1-BRIEF-after.md`, `a1-UPDATES.md` |
| 2 the four rules, one case each | PASS | new fact with scope evidence auto accepted; supersede unused silent; supersede used waits and lists the Share; preference waits (manual); resend of an `idempotency_key` adds nothing | `a2-digest.json` |
| 3 50 suggestions in a day, at most 5 human operations | PASS | 2 operations (read the digest, one `review:decide` batch for everything that waits), batch 0.047 s | `a3-human-operations.json` |
| 4 traceability to suggestion, credential, evidence | PASS | 43/43 accepted facts join to their suggestion, the credential that wrote it and a copied evidence block; 44 `fact_assert` overlays name the suggestion | `a4-trace-sample.txt` |
| OO never writes the Drive | PASS | the Drive tree is byte identical at the end | `l5-result.json` |

Not covered here (Desktop, phase two): the revision-mode view (F-S8), the batch-accept UI, the file write for an `editor`
Share (OO provides the diff draft `GET /suggestions/{id}/diff`, `writable` is true for an editor, `writer` is `desktop`).
Notification 2 s bound of `puppyone://share/<sid>/updates` is proved by `tools/mcp-gateway/tests/updates.rs` (mock engine), not
by this script.
