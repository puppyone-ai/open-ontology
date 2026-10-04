# Handoff

**APPROVE LIMITED** the unpublished PostgreSQL full import on `cc764cbc5f9d5d2cf6056a2c41ff2612161b5ff4`, tree `da85327b402378cfdc3b826300205569c43b3642`, parent `2996562ceb332cc0ce505c703e775615fa321e5b`.

The next session may treat `import_unpublished` as a reviewed composition: one target connection, a real snapshot, approved COPY, masked quality, one receipt transaction, `finish_complete`, and a stored `awaiting_publish` row. The requirement map is in `REPORT.md`. The fresh oracle is `linux/probe/live/outcome.txt`. Smoke exits are under `linux/smoke/`. Attempt 1 is `linux/probe/attempt-1/`.

Reuse a149 (`16ca2969`), a153 (`bedd6a2c`), a142 (`b5415544`), a148 (`d80472d8`), and a152 (`4ed22b8c`) for the reader, mapping, encoder, and installer, including the route that runs `0004` alone. Those blobs are unchanged on this commit except the additive `install_full_import` path. Do not rerun those matrices for this commit.

`fail_fence` left shard `a159take` in `full_import_run.state = open`. `finish` did not seal it. The contract's retire list is cancel, deadline, lost snapshot, and typed refusal. Details are in `FINDINGS.md`.

Do not treat this file as approval of publication, an Agent grant, a full type certificate, CDC, a control mirror, a job, a stream acknowledgement, or parent SP04. The `pg_read_all_stats` grant is not a security approval. a152's second-database bootstrap limit is unchanged.

Author evidence `549a82a0` is not this review's proof. No product, manifest, lock, spec, or ledger file was edited. Nothing was pushed. Own containers, volumes, network, and `/tmp/oo-a159-eef831aa-dba4-4af2-a78b-541bc9e0a159` are removed. `CLEANUP.txt` records that. Foreign containers were left in place. `docker prune` was not run.
