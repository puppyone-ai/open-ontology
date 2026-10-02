# a100 handoff

Independent IPC identity supplement. No push, no pull request, no release, no product commit.

Reviewed object: commit `6831d6e862303a0ce1c883925068078cbcb78e23`, tree `2f7fc8e8bb8ea34551235190e8e200e750358af9`, parent `77d957750e18c7e811e845a7717d54372b41839e`. ADR `docs/adr/ADR-P05-parser-prototype.md` stays Proposed (`9de08bb5e96555cf0fb39fbac476795b62404e9e528ffb12680d6c2a90a176b1`). Spike lock `c46ce7f6a90e7f2983eaa4bb1471fb45e275e38a220716175801d56981ce53b7`. Corpus blob `2ead1211bc596463550d4741862364fd45c589e4`.

This file supplements a091. It does not replace that review. a091 `raw/probe-keep.out` is still `fb6984490f9b1fa89945e21665e6194475d239ebba4f7e86ead3953f4a651a46`.

Decision: `identity_checks_held` on the production `admit_in_worker` identity cases in `review.md`. Full P5, R4, R5, R9, Gateway, OID/ACL/RLS authorization, native 17.7 versus server 17.10, network, source I/O, pid namespace, runsc, supply, amd64, and release stay unsigned.

## Oracle

Linux arm64, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Attempt 3 only: fetch 0, fetch-probe 0, offline debug build 0, `timeout -k 20 240 /target/debug/p5-identity-probe --probe` 0, inside 0, docker-run 0. Probe binary 33535112 bytes, SHA-256 `10620a272a86bea1301b3ce899528a49e8e1ace4ed791bd956e2e9dadfec6a6f`. `probe.out` SHA-256 `c8f6a6ce3b45bef1aaeb744fce599da1abf5ce94a96150c4219b432e036e6024`. Nineteen cases, `fails` 0. `probe.err` empty. No `test-hooks`, no corpus rerun, no Clippy, no release rebuild, no SQL to PostgreSQL.

Attempt 1 (apt under `CapDrop=ALL`, exit 100, docker-run 1) and attempt 2 (probe compile 101, probe skipped 127) stay under `raw/attempt1/` and `raw/attempt2/`.

Control reached `admit_in_worker` through `--supervise` and returned `ALLOW_SYNTAX` / `SELECT 1` with detail `a100-control` and node count 7. The in-process native `SELECT 1` comparison is detail empty and node count 3. Ready nonce, pid, and starttime spoofs, plus omit, unknown, and both duplicate orders, returned `INVALID_OUTPUT` before `KIND_ADMIT`. A parsed wrong decision nonce or pid cleared SQL and allow and kept detail `a100-control` and node count 7. The public API checks the decision nonce. Duplicate keys and an unknown decision `starttime` failed decode. Worker `starttime_matched` and `oid_acl_rls_proven` were not accepted as proof. Caller-hash mismatch was `UNTRUSTED_WORKER` before spawn.

Ready-reject proofs name the worker pid and starttime; those starttimes were gone (`proof_alive` 0) with `reaped` true and `containment=proven`. Foreign sleep pid 1852 starttime `548271` survived every case. Spoof pid 99999 was absent and was not that sleep. No false allow and no false reap.

## Cleanup

Uuid `5dd178d8-06bf-4ab2-aed9-66575a76475b`. `docker rm` of `oo-a100-msrv-5dd178d8`, `docker network rm` of `oo-a100-5dd178d8-net`, and `docker volume rm` of `oo-a100-5dd178d8-cargo` and `oo-a100-5dd178d8-target` each exited 0. No prune. Those names are gone. Other containers were left in place.

a098 is the root and connector writer. a102 is the jobs migration. a103 is the CLI read-only review. This task did not write those trees.

## Stop

Details, the case table, and the hash list are in `review.md`, `findings.json`, and `raw/hash-matrix.txt` (`24aa25814a993abf1fc02c6e0b648b3c5b2b5b102dd132b0480c8d59ad664162`).
