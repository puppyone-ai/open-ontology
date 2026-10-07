# Numeric query review handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a340`. Reviewer a340. Fixed product commit `21cc96241a841b47dbbcb116549030e38a643ba0`. Builder name `oo-a340-proof`, removed after `linux/proof/hostconfig.txt`. PostgreSQL `oo-a340-pg` id `9bb4a1b0693029a35d7235e656596cc4e1855376fd6d80c3e35035dd65fb5717` until own cleanup.

Verdict for child E46 / M1-D14-N: **APPROVE_LIMITED**. Parent M1-D14, M1-I01, M1-I02, the a317 100k fixture, the CSV decimal integration, and release stay open. No new unit count. No rework count. No release claim. No full SQL capability claim.

`linux/proof/docker-run.exit` and `proof.exit` stay 1 because gold attempt 1 returned 1. `phases/test-live.exit` is 0. `phases/gold-rerun.exit` is 0. `gold.txt` has `gold_match=1`.

## Owned paths

This review writes only `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a340/SP06/numeric-query-review/**`.

Product and repo root were read and archived. They were not edited. `.a340-work/` holds the archive and the private secrets. It is not part of the evidence commit. The evidence commit adds this directory only.

## Proof map

- `origin/recipes.txt` lists a327, a200, a204, and a199 origins with path and SHA-256, and the a340 deltas.
- `source/expected.sha256` is the eight archive hashes. `linux/proof/archive-sha256.txt` matches it. `SOURCE-INVENTORY.md` repeats them.
- `pg/` is the private TLS PostgreSQL 17.10 launch. `up.exit` is 0. `pg/actual/` is the post-proof `psql` capture. `capture.exit` is 0.
- `linux/proof-failed-1/` and `linux/proof-failed-2/` keep the failed attempt logs and the `inside.sh` that belongs to each attempt.
- `linux/proof/` is the live run. `test-live.exit` is 0. `gold.exit` is 1. `gold-attempt-1.txt` keeps the first oracle output. `gold.txt` and `gold-rerun.txt` are the corrected match.
- `REPORT.md` and `FINDINGS.md` are the result narrative.
- `tools/sha256.rs` is the std-only inventory tool, origin hash `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e`. `tools/gold.rs` is the corrected oracle.
- `cleanup.txt` is the own-resource removal log, written before the SHA-256 walk.

## What the next reader can rely on

The passing builder compiled the pipeline, builder, and gateway bins from the fixed archive, ran the pre-live lib, decimal, row-filter, and Clippy checks, then ran `a327_live::live_numeric_proof` against `oo-a340-pg`. The six amount and wide cells are exact decimal JSON. Unbounded numeric and numeric(39,4) provision exited 7 before the mapping grant. Hidden exited 6. The pre-analyze query exited 5. Details and the limits are in `FINDINGS.md`.

executed_sha256 `f03136a329352cbec8ec06fe74cbac7147b26223b25812b17e94501570819326` and config_digest `47280e45c50fdd7d8a306b98f1265321a17465af3c29afe7df74a298b2d4129c` belong to this cluster. original_sha256 and schema_hash match author live-4.

## Cleanup

`cleanup.txt` records, in order: `docker stop` and `docker rm` of `oo-a340-pg` (`9bb4a1b06930`); `docker volume rm` of `oo-a340-eef831aa-cargo`, `oo-a340-eef831aa-target`, and `oo-a340-eef831aa-pgdata`; `docker network rm oo-a340-eef831aa-net`; removal of `.a340-work`. Each command exited 0. No prune. The commands named only those a340 resources. The `oo-a337` name filter was empty before and after. After that log, name-filtered `docker ps`, `docker volume ls`, and `docker network ls` for `oo-a340` are empty, and `.a340-work` is absent.

## Manifest

`linux/inventory/` runs after `cleanup.txt`, on the pinned Rust image, with `--network none`, `--cap-drop ALL`, label `oo.kind=hash`, 1 GiB, and 1 CPU. It compiles `tools/sha256.rs` with `rustc --edition 2021` to `/usr/local/bin/sha256` and writes `SHA256SUMS` plus `SHA256SUMS.sha256` at this directory. The tool skips those two output names. `/tmp` inside this image is noexec, so the binary is not placed there.

`linux/inventory/capacity.txt`, `linux/inventory/image.txt`, and `linux/inventory/invocation.txt` are written before the container starts, so they are inside `SHA256SUMS` when the walk succeeds.

Files the host writes after the walk, and therefore absent from `SHA256SUMS`:

- `linux/inventory/docker-run.exit`
- `linux/inventory/hostconfig.txt`
- `linux/inventory/container-removed.txt`

The evidence commit object is not a file in this directory.
