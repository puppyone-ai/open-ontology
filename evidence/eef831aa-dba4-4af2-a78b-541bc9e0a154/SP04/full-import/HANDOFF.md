# Handoff

Source is `cc764cbc5f9d5d2cf6056a2c41ff2612161b5ff4` on `master`. Tree `da85327b402378cfdc3b826300205569c43b3642`. Parent `2996562ceb332cc0ce505c703e775615fa321e5b`. Fixed base `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c` is an ancestor. The evidence commit that cites this file is the next commit. Do not reopen `0000` through `0004`. Do not treat `full_import` as a publication or as permission to classify.

## What is public

`import_unpublished(&mut PgConnection, ImportRequest) -> RunSummary`. The only success state is `AwaitingPublish`. `install_full_import` and `verify_full_import_readiness` are separate from `install` and `verify_readiness`. `0005` digest is `a9cef2013019784857353ff4fe060350d71a4e9ac4ab18551b992a7a9b52ecb6` and is not one of the four frozen installer checksums. Callers pass one idle target connection. That connection holds the shard lock and is the connection used for `COPY` and the receipt.

`install_full_import` grants `pg_read_all_stats` to `oo_target_owner` `WITH INHERIT TRUE, SET FALSE` so the security-definer fence can read the caller's `backend_start`. Runtime and Agent are not members. The grant is not the fence and is not a security proof.

## What the green probe already proved

One private PostgreSQL 17.10. Installer live kept the frozen outcomes and added `0005` install, replay, `OO001`, and runtime refusal. The public entry imported two tables to `awaiting_publish` (4 rows, generation 1), denied the agent, kept source writes at 0, retired cancel, deadline, snapshot loss, and the oversized table, refused `jsonb`, bounded backpressure at 2 chunks, read a coherent second cut (2 then 3), blocked the same shard, imported another shard, incremented on takeover, left rollback and crash-before absent, replayed crash-after, and aborted a conflicting payload. Scoped Clippy `-D warnings` exited 0 on Linux 1.94. Raw lines are in `REPORT.md` and `verify/live/outcome.txt`.

## What is still open

Control mirror, job, and stream acknowledgement are separate later tasks. The live fixture is a representative type set. `jsonb` is refused and the other PostgreSQL types were not certified. The route that runs `0004` alone, when fence v3 is already valid and `physical-mapping/v1` is absent, remains the open installer case from a150.

## Attempts

`verify/attempt-1-lib-101` through `verify/attempt-9-clippy-lib` are preserved failures. Attempts 8 and 9 printed `full_import_live_ok` and then failed clippy. They are not the acceptance run. The acceptance run is the verify root with every phase exit 0.

## Containers

a154 containers, the a154 network, and the a154 cargo, target, and postgres volumes were removed after the green probe. The private `/tmp` directory that held the archive, password files, and TLS keys was removed. The passwords were not in the evidence tree. `docker prune` was not run. No foreign container was removed.
