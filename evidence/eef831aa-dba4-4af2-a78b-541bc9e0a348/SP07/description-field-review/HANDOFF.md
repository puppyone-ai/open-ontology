# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a348`. Producer commit `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. Parent `93f5b8aea78d0d745334da0bc5352afdb86412e2`. Tree `c959efb16348a48f682a4444a6366e44bf70c0b1`. Decisive run `linux/proof-3`, container id `989aea8200e0d53f3cdb177bc86101a7415abf99feebe118fd3cb31b77757d25`, `docker-run.exit` 0.

This file records the independent review of the column-field producer. It does not close E47. It does not accept SP07. It does not accept the parent goal. It does not add a countable unit. Actual Builder integration is still required before E47 or SP07 can be accepted. a328 was not used as source and was not edited.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs.txt`
- `overlay/a348_sealed.rs`
- `linux/proof-1/ATTEMPT.txt` and `linux/proof-1/test-a348.log` for the compile failure
- `linux/proof-2/ATTEMPT.txt` and `linux/proof-2/test-a348.log` for the three passing tests and the Clippy rejection
- `linux/proof-3/` for the 0 exits, Clippy, lock subset, and product byte compare
- `origin/recipes.txt`, `CLEANUP.txt`, `MANIFEST.sha256`

## Decisive exits

`linux/proof-3/docker-run.exit` 0. Every phase file under `linux/proof-3/phases/` is 0. Author counts before the oracle existed: candidate 10, CLI 5, lib 2, review 21, pack 21, provider 23. Oracle: 3 passed. Lock summary `registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true`. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a348-eef831aa-net`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Rust 1.94.0 aarch64. PostgreSQL was not started. `cargo-update-invoked.txt` is `no`.

`linux/proof-3/capacity.txt` recorded `own=0 global=0 host_cargo=0` before start. The launch script removed `oo-a348-proof3`. `CLEANUP.txt` records removal of the private network and both volumes.

## What a later Builder review still has to show

The producer archive already contains the v2 column cards, the review of label and description, and the pack reseal. This review checked those with fixture tests. A later review that wants E47 or SP07 acceptance has to exercise the real Builder integration on its own evidence. That work is outside this directory.

Publication of a new column description still requires `settings.ontology.publish.include_unreviewed` to contain `column_descriptions`. The role text is then prefixed `UNREVIEWED:`. A curated `column.review` is not that grant. The token `descriptions` is refused.

## Owned path

The commit for this review contains only `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a348/SP07/description-field-review/`. Product bytes at `c23db28dc27f5cf20741dd36e8a8e88129ad1715` were compared after the oracle file was removed. `linux/proof-3/product-diff.txt` is empty.
