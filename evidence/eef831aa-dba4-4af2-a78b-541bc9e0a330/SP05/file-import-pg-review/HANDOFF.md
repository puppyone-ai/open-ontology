# file-import-pg review handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a330`. Reviewer a330. Fixed product commit `15558917e2c5be12b74dd1d64fb542b7d05a03af`, tree `140fddc79768fd4d1c5b43c4f46ba2acba6cc5bf`. Passing proof container `21a07430666a91468e3c32a33502c76846cd26c792cdfc5e8f3b5bd393eec4ca`. Repo head recorded by that launch: `53931278e69d723d0ce38961ded897fd6e41d5d3`.

Verdict for child FILE-D02 / E39: **APPROVE_LIMITED**. Parent SP05-W05 and AT-FILE-06 stay open. No new unit count. No rework count. No release claim.

## Owned paths

This review writes only `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a330/SP05/file-import-pg-review/**`.

Product, root, and `tools/file-import-pg` were read and archived. They were not edited. The tool lock was not copied back. The evidence commit adds this directory only.

## Proof map

- `origin/recipes.txt` lists a204, a200, a199, and a308 origins with commit, path, and SHA-256, and the a330 deltas.
- `pg/` is the private TLS PostgreSQL 17.10 launch. `up.exit` is 0. Container id `e24b368e74e7ebe5a0c3a8302713aef6a457a6c40a3a04416d0ef6607e142dae`. `pg/reset.txt` records the in-place role and database drops between attempts.
- `linux/proof-failed-1` through `linux/proof-failed-4` keep the failed attempt logs, `ATTEMPT.txt`, and the oracle source that belongs with each note.
- `linux/proof/` is the passing run. Every phase exit is 0. `docker-run.exit` is 0. `refuse.stdout` is 6 passed tests in 0.01s. `live.stdout` is 1 passed test in 5.03s. `review.stdout` is 1 passed test in 5.63s. `oracle.txt` ends at `oracle_end=1`.
- Worker SHA-256 `48b6ab508a044a0419ae476f0aacfae41683392abad2002e65dbf8fa56d58054`. Tool lock SHA-256 `9874a7ff5c09b333ab1392829334d648f42340a3ba40d7db167c4690c081e89d`. Per-file crate hashes are `linux/proof/source-sha256.txt`.
- `REPORT.md` and `FINDINGS.md` are the result narrative.
- `cleanup.txt` is the own-resource removal log. It was written before the manifest walk.

## What the next reader can rely on

The passing builder compiled `oo-file-parser-worker` from the fixed archive, ran the author's six refuse tests and one live test, then ran an independent overlay. That overlay imported CSV, TSV, and JSONL through the worker into a LOGGED unpublished shadow, checked int8 min and max, numeric `-1.25` / `0.00` / `1.50`, quoted empty, token null, JSON null versus the text `null`, a missing JSON field, a truncated object, a complete object with no newline, replay, hash conflict, hash drift before the child, SQLSTATE `42501`, and `pg_terminate_backend` followed by one retry. Details and the limits are in `FINDINGS.md`.

## Cleanup

`cleanup.txt` records, in order: `docker stop` and `docker rm` of `oo-a330-pg`; `docker volume rm` of `oo-a330-eef831aa-cargo`, `oo-a330-eef831aa-target`, and `oo-a330-eef831aa-pgdata`; `docker network rm oo-a330-eef831aa-net`; removal of `/tmp/oo-a330-eef831aa-dba4-4af2-a78b-541bc9e0a330`. The network had only `oo-a330-pg`. The proof container was already gone. No prune. No foreign container, volume, or network was removed. After that log, name-filtered `docker ps`, `docker volume ls`, and `docker network ls` were empty.

## Manifest

`linux/inventory/launch.sh` runs after this file, on the pinned Rust image, with `--network none`, `--cap-drop ALL`, 6GiB, and 1 CPU. It compiles `tools/sha256_manifest.rs` with `rustc --edition 2024 -D warnings` and writes `MANIFEST.sha256`. The tool skips that output path. Console for that launch is `/tmp/oo-a330-hash-console.log`, outside this directory, so it is not in the manifest.

Files the host writes after the walk, and therefore absent from `MANIFEST.sha256`:

- `linux/inventory/docker-run.exit`
- `linux/inventory/hostconfig.txt`
- `linux/inventory/container-removed.txt`

`linux/inventory/capacity.txt`, `linux/inventory/image.txt`, and `linux/inventory/invocation.txt` are written before the container starts, so they are inside the manifest when the walk succeeds. The evidence commit object is not a file in this directory. These files do not contain that commit hash.
