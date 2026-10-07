# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a364`. Reviewer: one Grok 4.7 session, no subagents and no worktrees. Product path `tools/selfbuild-suggestions` was read and tested. It was not modified.

## Decision

APPROVE LIMITED E50-D1/1 candidate production only, at `d22187ea0fce8e420323660841f2da3b447d5de8`.

Withheld: SP13 parent, authentication, certification, publication, S6 entity merge, parent-plan acceptance, and goal acceptance. Do not increment those acceptance counts from this packet. Do not treat `cand_7cf18e19bea87e5f3599782605e88982` or any other candidate id in `live/outcome.json` as an accepted entity.

`knowledge_verified` is false. The positive proof is one mapped text row, `a364-mapped-row`, not a 100k load.

## Where the proof sits

- Receipt: `live/capture/request-2.stdout` and `live/capture/request-2.audit.jsonl`
- Guard refusal: `live/capture/request-1.exit` is 6, stdout empty, no audit file
- Outcome: `live/outcome.json`, exit `live/outcome.exit` is 0
- Checker: `probe/oracle.rs`, result `live/oracle.txt` (`oracle ok`)
- Gold SQL and row line: `gold/select.sql`, `gold/probe.sql`, `gold/row-line.txt`
- Database apply record: `pg/apply.txt`
- Focused tests: `build/test.txt`, `build/independent.txt`, `build/clippy.stderr`
- Failed checker attempts: `failed-attempts/live-1/`, `failed-attempts/live-2/`
- Exit index: `EXITS.md`

The scripted value `scripted-not-a-receipt` in `probe/independent.rs` is a unit-test stand-in. The receipt is the two capture files above.

A judge-only rerun set `OO_A364_JUDGE_ONLY=1` so the second container would not call produce again. The receipt hashes did not change. See `live/capture-sha256-before-judge.txt` and `live/capture-sha256-after-judge.txt`.

## Reproduce limits

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `--platform linux/arm64`, `--pull never`. One rust builder at a time for this task, 6 GiB, 1 CPU. One private PostgreSQL, 1 GiB, 1 CPU. Query-pg source archive commit `7a6f7eac8b8041eae215c087d1a80930437871ce`.

Do not reuse another UUID's volumes, cargo home, or binary. Do not mount the host home, the docker socket, or host networking. Secrets for this run lived only under `/tmp/oo-a364-sbs-secrets` and are removed with the owned cleanup. They are not in this directory.

`oo-a361-pg` and `oo-a359-pg` were left running. This review did not prune Docker.

## Coordinator

This worker did not wait 18 minutes and did not poll. The coordinator's single 18-minute wait and single handle observation are outside this packet. CLEANUP.md has the one snapshot taken while removing a364 resources.

Workspace HEAD moved during the review because other sessions committed. The product comparison that matters is `git diff d22187ea0fce8e420323660841f2da3b447d5de8 -- tools/selfbuild-suggestions`, which was empty when this packet was written. The evidence commit that follows contains only `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a364/SP13/selfbuild-suggestions-review/`.
