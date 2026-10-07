# SP07-W04 PostgreSQL builder report

Session `eef831aa-dba4-4af2-a78b-541bc9e0a328`. Actor a328. Fixed product baseline `8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8`. `parent_acceptance` is false. This report does not accept SP07, the full plan, or a release.

Owned paths:

- `tools/pg-builder/**`
- `testing/real-db/pg-builder/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/**`

## What ran

The reusable CLI is `pg-builder`. Catalog capture calls the existing `oo-pipeline` binary as `catalog-pg`. Dataset candidates use the existing description-candidate compiler. Field candidates are staged in this crate because that compiler admits dataset fields only. The seam id is `description-candidate.dataset_only`. The model does not grant, certify, classify, or publish. `approve` writes `authenticated` false and `parent_auth` required.

Linux arm64 Rust 1.94 image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. No image pull. One builder at a time, 6GiB, 1 CPU, pids 256, no privilege, no host network, no Docker socket. `cargo-update-invoked=no`. The nested lock was checked against the root lock that existed at build time. That root lock SHA-256 was `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` before and after focused-4 and the catalog container. Later actors changed the workspace `Cargo.lock`. This unit did not edit it. Its hash at report time is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`.

Recipe origins: a317 up script `d53018ef8ee844be825643d4cdb31c7be49cf67fdf144b7d5842f230b82ad73c`, a317 wrap `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb`. Launch recipe hashes recorded with the smoke tree: a200 `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163`, a204 `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`, a199 `c59441884fcd7ce54b8e6cb11a6c632cf5dc8ba757d112145fbf20ede8d83ac3`.

## Linux proof

| Tree | docker-run.exit | Result |
| --- | --- | --- |
| `linux/smoke` | 1 | Rust 1.94 image has no clippy component. Preserved. |
| `linux/smoke-clippy` | 0 | Clippy installed inside that container only. |
| `linux/focused` | 1 | First product compile failed in `generate.rs`. Preserved. |
| `linux/focused-2` | 101 | Tests failed to compile; clippy warnings. Preserved. |
| `linux/focused-3` | 101 | 10 integration tests passed; one clippy lint remained. Preserved. |
| `linux/focused-4` | 0 | `pg-builder` tests 10 passed, `pg-fixture` tests 2 passed, clippy `-D warnings` clean. This proof is earlier than the additive `command_bytes` and `prepared_bytes` seal fields. |
| `linux/catalog` | 0 | Same 1.94 builder compiled `oo-pipeline` and the later `pg-builder`, then ran both catalog passes. Tests were not re-run in that container. |

Host `pg-builder` debug binary SHA-256 `6440afd430126e83c37e7998402e2891305306bf776c0eb7f35e994ed024ae04`. Host rustc follows the workspace 1.98.1 toolchain. The MSRV proof is the 1.94 container. The binary was removed with the private tmp tree after that hash was recorded.

## Private database

Container `oo-a328-pg` loaded the public synthetic fixture: party 2000, account 8000, entry 90000. `ANALYZE` left `reltuples` at those counts. Reader `oo_a328_reader` had `SELECT` only. `grants-before.txt` and `grants-after.txt` match (`grants_unchanged=yes`, 24 lines). Server 17.10, autovacuum off, TLS on, SAN DNS `oo-a328-pg`. No host port. Passwords were not printed and are not in this tree.

Catalog pass exits, both passes, for `catalog`, `profile`, `comments`, `seal`, `cards`, `curated`, and `approve`: 0.

Pass 1 evidence hash `992617a1ff52ba9509a884e63977d1256bcaae231850bcd170c2f65dd10c9b50`. Pass 2 evidence hash `2c66f95d485a9df43d08edd0c8f097c7a283ed15b4511735b6482438a764369b`. Schema hash both passes `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`. Seal `prepare_status=request`. Pass 1 `command_bytes=45766`, `prepared_bytes=5343`. Pass 2 `command_bytes=45768`, `prepared_bytes=5344`. Sample policy `none`. Histogram and most-common values are withheld. Table comments are `untrusted_data`. The pass 2 party comment is the injection sentence and was not treated as an instruction by the catalog.

## Live model chain

Attempt 1 is preserved under `live/llm-attempt1-input-read/`. Every generate, review, and pack exited 1 with `input.read` because the config path was a bare filename and the process cwd was not `live/llm`. No socket was opened. The script now passes absolute config paths.

Attempt 2 wrote generation documents and exited 0 for every generate. Config origin for generate is `http://127.0.0.1:8317`, path `/v1/chat/completions`, model `grok-4.7`, bearer file only. Fault injection used `http://127.0.0.1:9`. Declared price status `unknown`.

| Call | Exit | Outcome | Cache | Store | Socket | Attempt | Refusal | Latency ms | Input SHA-256 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| timeout (deadline 1 ms) | 0 | structural_draft | miss | not_stored | opened | 1 | timeout | 2 | `cbd96275d3767ab13b704bc979e56e731bba0f1c2c23bcc5051a998af6da8458` |
| truncated config | 0 | structural_draft | miss | not_stored | opened | 1 | timeout | 30003 | same as timeout |
| unavailable | 0 | structural_draft | miss | not_stored | opened | 1 | unavailable | 0 | same as timeout |
| success | 0 | structural_draft | miss | not_stored | opened | 1 | timeout | 30003 | same as timeout |
| second process | 0 | structural_draft | miss | not_stored | opened | 1 | timeout | 30003 | same as timeout |
| pass 2 injection | 0 | structural_draft | miss | not_stored | opened | 1 | timeout | 30002 | `5b490cf19cebcec1833717e81fb784ca3296abc61a6365c418a4fbb77ff7828a` |

Request input hash for the pass 1 calls: `d6e5c34b4302400c82b2f7f1c0a3043d850082ca9487e7dee61e38dd43e209bc`. Pass 2 request input hash: `833fe4e9cbec9b47e5d3ad45744cd1919af24d577b5d119a6b86e4f71f0ec08b`. Response hash on these drafts is `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`, the SHA-256 of the empty raw buffer. No model body was stored. `repair=not_performed`. Usage is null. Declared cost status `unknown`, amount null, invoice false, billing certificate false. The cache directory had 0 files.

`summary.txt` contains `cache_hit_same_candidates=true` because the script compared two timeout drafts. The following lines in that file record `cache_hit_proof=false`. The second process opened a socket. It is not a cache hit.

A separate loopback probe, token kept in the Authorization header, showed a short completion returning in about 1.5 s and a prepared-request-sized chat whose first byte arrived at 97280 ms. The product deadline is 30000 ms, so the product exchange ended as timeout. The probe did not decode chunked framing and its late body did not parse; that body was not turned into a candidate. Details are in `live/llm/endpoint-observation.txt`.

Review of the success document exited 1: `review.blocked`. Pack exited 1 with the same code. The role that names `national_id` also stopped at `review.blocked`, so this run did not reach `classification.missing`. The injection review probe exited 1 and wrote no review file. `curated-before.sha256` matches `curated-after.sha256`. No review file, no pack file, no role ACL change, no publication, no compiled pack.

`bearer_scan=clean` for the evidence tree against the customer token file. A later scan of the three owned trees found the 6-byte token only as a substring of the public hexadecimal alphabet constant and a synthetic numeric fixture literal. Those hits are not copied credentials. The token was not written into config JSON, prompts, logs, argv, or this report.

## Cleanup

Removed container `oo-a328-pg`, volumes `oo-a328-eef831aa-pgdata`, `oo-a328-eef831aa-cargo`, and `oo-a328-eef831aa-target`, network `oo-a328-eef831aa-net`, and `/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328`. The customer token file was left in place. No prune and no foreign container cleanup. Recorded in `pg/cleanup.txt`.

## Seams left open

- `description-candidate.dataset_only`: column candidates are staged here and are not applied by description review or description pack.
- Shop `entry.amount` is `numeric(18,4)`. The separate numeric query seam remains. This unit did not edit `oo-gateway`.
- The production description provider still requires HTTPS and a 16-byte minimum bearer. The 6-byte token is accepted only by `loopback_http_local_dev`.
- The live model did not return inside 30000 ms, so validated S5 table and field candidates were not stored, and the live role pack was not compiled.
- `parent_acceptance` is false.
