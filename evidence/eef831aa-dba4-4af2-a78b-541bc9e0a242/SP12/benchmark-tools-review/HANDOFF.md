# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a242`. Review only. Archive `4b15249b70ee38d5a4d21c546b1329e6532ccfed`.

## Verdicts to carry

- B-D01 result comparator: APPROVE_LIMITED.
- B-D02 query-result adapter: APPROVE_LIMITED. Truncated `succeeded` with `end_audit_ok` true is adapter completion `complete`. Full gold mismatches. Prefix gold matches. The runner must refuse truncation on its own.
- B-D03 trial statistics: APPROVE_LIMITED. pass^k uses the question denominator. Reannotation stays in that denominator. The oracle is the small seed recorded in `linux/proof/harness/bootstrap-indexes.txt`.

Do not mark the SQL runner, a full agent, a customer benchmark, the parent SP12 item, or a release as accepted from this directory. Do not edit the acceptance-counting spec from this review. B-D04 stays on its own frozen commit.

## What the next runner has to do

When an adapter actual is complete and the query-pg summary says `truncated` true, the trial fails even if a prefix gold compares equal. This review measured that split. It did not execute `benchmarks/sql-runner`. That path is not in the fixed archive.

## Resources

Owned names: network `oo-a242-eef831aa-net`, volumes `oo-a242-eef831aa-cargo`, `oo-a242-eef831aa-target`, `oo-a242-eef831aa-pgdata`, containers `oo-a242-proof` and `oo-a242-pg`, tmp `/tmp/oo-a242-eef831aa-dba4-4af2-a78b-541bc9e0a242`. `cleanup/cleanup.sh` removes those and does not prune. Foreign containers stay. Pinned images stay.

The successful builder was removed by `linux/proof/launch.sh`. Proof id `1989f683e955f8b3d6e0e2fc0540963d5288de09a2b6e78328800c1ed31733e3`. PostgreSQL id was `92b55633ef27e7e2c37de9cf9e8a37a409d99c186ecafd0e17eabaf40e894e53`. `cleanup/cleanup.sh` then removed that container, the three private volumes, the private network, and the tmp directory. `cleanup/cleanup.log` ends with `cleanup_ok`. Images and foreign containers were left in place.

## Reproduce

Smoke must already show `linux/smoke/docker-run.exit` of 0. Remove or archive `linux/proof/console.log` before a new launch; the launcher refuses to overwrite it. Then:

`bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a242/SP12/benchmark-tools-review/linux/proof/launch.sh`

Images are local pins. The script does not pull. One builder, 6 GiB, one CPU. One private PostgreSQL. `RUSTUP_TOOLCHAIN=1.94.0`. The archive's `rust-toolchain.toml` is renamed to `rust-toolchain.toml.repo` inside the private extract.

Raw exits, attempt logs, harness stdout, and the truncation boundary are under `linux/proof/`. Source trees are under `source/blobs/`. `SHA256SUMS` is the a199 hasher output.
