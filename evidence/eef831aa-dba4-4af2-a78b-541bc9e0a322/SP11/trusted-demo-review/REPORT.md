# OPS-D01 trusted-demo independent review

Verdict: **APPROVE_LIMITED** for `deploy/trusted-demo` at tree `1b6e8229107039059a94b7bd65a1c59fc198cc96`, commit `895864a5bf64e50cef84075d20fd7b0c9e41c264`. Reviewed CLI binaries were built from the immutable archive `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a322`. This directory is the independent review of the combined public Docker demonstration, including resource identity. The author records are `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a305/SP11/trusted-demo` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a312/SP11/trusted-demo-identity`. Those records are not this review. No product file, root lock, connector manifest, spec, or ledger was edited. This file does not accept parent SP11-W06, M1, I01, I02, or a release.

The receipt fields are the scope boundary: `human_approval=not_claimed`, `authentication=not_claimed`, `authorization=not_claimed`, `semantic_publication=missing`, `natural_language=unsupported`, `model_provider=unsupported`, `harness=not_performed`, `m1_i01=not_accepted`, `m1_i02=not_accepted`, `parent_acceptance=false`. There is no HTTP endpoint, Compose stack, enterprise connection, or model call in this command.

## Result

The frozen entry was run from a private checkout of `895864a5`, not from a later worktree edit. The operator manifest is `boundaries/good.json`. Owner `a322` is the last four characters of run id `eef831aa-dba4-4af2-a78b-541bc9e0a322`.

`start.sh --manifest boundaries/good.json --public-fixture` exited 0. Host stdout:

```
postgres_ready name=oo-a322-pg alias=oo-a322-pg
fixture_loaded
restart_postgres
postgres_ready name=oo-a322-pg alias=oo-a322-pg
fixture_preserved
```

Host stderr was empty. `linux/product-host/public-fixture.exit` is 0. The copied product tree is `product/`. `product/linux/public/docker-run.exit` is 0. `product/linux/smoke/docker-run.exit` is 0. `product/linux/public/drive.log` is `ready_for_trusted_query`. `product/live/smoke/smoke-ok.txt` is `role_pack_unchanged` and `query_rows_repeated`.

Receipt `product/live/receipt.json`, protocol `oo.deploy.trusted-demo-receipt/v1`, status `ready_for_trusted_query`. `project=open-ontology`, `owner=a322`, `run_id=eef831aa-dba4-4af2-a78b-541bc9e0a322`, `source_id=public_shop`, `fixed_commit=25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`. `config_identity_sha256=b2cf4b149485564b18fc8f13986392694b0c256b3f8a89132a6351f9c8cf338d`. `schema_hash=da877f8bc78b7a9a14b8648004bfd904d8bc89b774bbca1253c1758927a74a2f`. Role packs: north `52f0ced1c75d9c251cc453396d09d8ac8af032b3be91ce89f50e713618374e42`, narrow `24972b46b61624fe13995774d574b2cc55747b608d09615fda68b53a187bfbac`.

North customer rows from `product/live/run/steps/query-north-customer.stdout`, exit 0, `returned_rows=3`, `original_sha256=e617988aaadeab6eb1c9302a033a83c2b3ee7df6133fc005b0d1de9079837400`:

- customer 41, display `丙号`, active true
- customer 42, display null, active false
- customer 43, display `丁铺`, active true

`product/live/smoke/north-customer.stdout` is byte-identical and carries the same original hash. That hash is the same three-row digest recorded by the a312 run. The schema hash matches the a305 and a312 runs. The config identity and both role-pack digests differ, because this manifest's host, databases, and role names are `a322`.

Inspect `product/live/run/outcome.json` is `phase=inspect`, `status=awaiting_review`, `source_cut=48`, `target_fence=1`. Apply `product/live/run/apply-outcome.json` is `status=ready_for_trusted_query`, `analyze_published=false`, `database_release=false`. Hidden column `shop.customer.private_note` has reason `classification.absent`.

Query exits in `product/live/run/steps/`:

| Step | Exit | Stderr |
| --- | --- | --- |
| query-north-customer | 0 | |
| query-north-orders | 0 | |
| query-north-count | 0 | |
| query-narrow-customer | 0 | |
| query-narrow-orders | 0 | |
| query-north-before | 5 | `query-pg: policy` |
| query-north-hidden | 6 | `query-pg: guard` |
| query-north-insert | 6 | `query-pg: guard` |
| query-north-pos | 6 | `query-pg: guard` |

`柜员甲` occurs in the north order stdout and does not occur in the narrow order stdout. `private_note` does not occur in the north customer stdout. `甲注评` occurs 0 times in `catalog.json` and `captured-draft.json`.

The bad review `product/live/bad-run/apply-outcome.json` is `status=failed`, `step=reviewed-effective-oos`, `detail=exit`. The child stderr contains `decision.unknown`. Step stems in that directory are `import-not-ready`, `init-pg`, `import-pg`, `catalog-pg`, `captured-pg-draft`, `review-pg-draft`, and `reviewed-effective-oos`. There is no column ACL, mapping reader, analyze, role pack, or trusted query file there.

Secret scans: `product/secret-scan.txt` is `secret_absent files=257`. `product/smoke-secret-scan.txt` is `secret_absent files=288`. No `.pass`, `.pem`, or `.key` file is in this evidence tree. The manifest and the receipt contain no `password` field. Password files lived under `/tmp/oo-a322-eef831aa-dba4-4af2-a78b-541bc9e0a322/secrets` and were removed with `--clean`.

## Fixture tests and the real chain

These are separate.

Pre-overlay smoke, before any product container, against a `git archive` of `25dfc902` with `live_chain` left ignored: `linux/preoverlay/test-count.txt` is `passed=24 failed=0 ignored=1`. `linux/preoverlay/test.log` contains `test live_chain ... ignored`. `duplicate_decisions_refuse_before_review` and `cli_failed_child_keeps_marker_and_static_stderr` passed. Clippy `--all-targets -- -D warnings` for that manifest exited 0. Driver tests were `7 passed; 0 failed`. Driver Clippy `-D warnings` exited 0. `linux/preoverlay/docker-run.exit` is 0. `linux/preoverlay/smoke-ok.txt` says `chain=not_run`. Toolchain text is `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

The product command repeated that smoke inside `product/linux/public/` before it built workspace binaries. `product/linux/public/test-count.txt` is again `passed=24 failed=0 ignored=1`, with the same ignored `live_chain` line. Its 33 phase exits are 0. The real chain is the PostgreSQL run after that smoke: import, catalog, explicit fixture review, role pack, column ACL, and `query-pg`. The restart is `product/linux/smoke/`, whose 7 phase exits are 0. `product/linux/public/cargo-update-invoked.txt` is `no`.

Driver source SHA-256 is `60f2ba592e456418e6da94c90e0a4815aa69299bc62adc74046ed07432b9aef1` (`linux/preoverlay/deploy-sha256.txt`, `./src/drive.rs`). Built artifact SHA-256 is `0e0e3a8c90c9d67526c35cd720d7daab30fa69619d04676da4ca2167e23f051c` (`product/linux/public/bin-sha256.txt`, `/cargo-home/bin/drive`). `tools/sha256_manifest.rs` is `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. Those three digests match the a312 record. The onboarding crate walk is `linux/preoverlay/onboarding-sha256.txt` (`files=16`). The frozen demo walk is `linux/preoverlay/deploy-sha256.txt` (`files=23`).

## Boundaries

`boundaries/summary.exit` is 0. Each refusal exited 2 from the frozen `start.sh` and did not create an `a322` or `a305` container, volume, private tmp, or evidence directory:

| Case | Exit |
| --- | --- |
| no args | 2 |
| `--public-fixture` without `--manifest` | 2 |
| missing file | 2 |
| symlink | 2 |
| owner `a321` with this run id | 2 |
| owner `a322` with run id ending `a321` | 2 |
| uppercase run id | 2 |
| second line | 2 |
| extra key | 2 |
| wrong `fixed_source` | 2 |
| carriage return | 2 |
| 513-byte file | 2 |

`boundaries/good-parse.stdout` matches `boundaries/good-parse.expect`. The historical `a305` line parses and derives `oo-a305-pg`. It was not passed to `--public-fixture`.

After the ready run, a second `--public-fixture` exited 2. Stderr refused the existing run directory, private tmp, network `oo-a322-eef831aa-net`, volumes `pgdata`, `cargo`, and `target`, and container `oo-a322-pg`. Volume timestamps stayed `2026-10-07T12:02:52Z` for `pgdata` and `2026-10-07T12:02:55Z` for `cargo` and `target`. `--stop` exited 0 and removed container `c067edebf700`. The same timestamps remained. `--clean` exited 0, removed those three volumes, and printed `cleaned_owned_private_resources`. The network and `/tmp/oo-a322-eef831aa-dba4-4af2-a78b-541bc9e0a322` were removed. A later `--public-fixture` exited 2 because `product/live/run` remains. A later `--clean` exited 0.

## Builders and PostgreSQL

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. One builder at a time. Each new builder was 6 GiB (`Memory=6442450944`), 1 CPU (`NanoCpus=1000000000`), `Privileged=false`, `CapDrop=["ALL"]`, `OOMKilled=false`. Pre-overlay capacity was `own=0 global=0 host_cargo=0`. No host `cargo` or `rustc`. No pull, prune, host network, published host port, Docker socket, or host-home mount.

| Run | Container | docker-run.exit |
| --- | --- | --- |
| preoverlay | `9363b2fce9c8671fe72c8b7543b12c7b25efb436fc1202aab0ff6e482cfbe0bf` | 0 |
| public | `93315b61e49141414d2216c5f650c005775c9f9c18db1e66e260af37a54df224` | 0 |
| restart smoke | `045d867a3803b8afd0ba4703dcdcc07c215fbad7be46d01761186186cc6c1f3e` | 0 |

PostgreSQL `oo-a322-pg` id `c067edebf70017fd1708c1a239339619906362d280ba26d6c721a5f8931e12e2`, 1 GiB, 1 CPU, not privileged. `product/pg/ports.json` is `{}`. `product/pg/server.txt` records `server_version=17.10`, `autovacuum=off`, `ssl=true`. `product/pg/restart-state.txt` is `restart_state=present`. `product/pg/caps.json` drops `ALL` and adds `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, and `SETUID`. Labels on the server, three volumes, and network were `oo.project=open-ontology`, `oo.owner=a322`, `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a322`, and `oo.fixed=25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`.

The preoverlay network `oo-a322-eef831aa-pre-net` and volumes `oo-a322-eef831aa-precargo` and `oo-a322-eef831aa-pretarget` were removed after that smoke exited 0. They are not the product volume names.

## Inventory

`SHA256SUMS` is the Safe Rust walk of this directory after this report, `FINDINGS.md`, and `HANDOFF.md`. The walker is the frozen `tools/sha256_manifest.rs`. It skips its output path. One private builder, network `none`, 6 GiB, 1 CPU, compiles that file and runs it. `linux/hash-manifest/launch.sh` is part of the walk. `linux/hash-manifest/console.log`, `hostconfig.txt`, `mounts.txt`, `docker-run.exit`, and `container-removed.txt` are copied after the walk, so those logs are not members of `SHA256SUMS`. The console holds `files=` and the empty and `abc` self-checks.

## Cleanup

Owned cleanup used the frozen `start.sh --manifest boundaries/good.json`. Exits are in `cleanup/`. `cleanup/stop.exit` is 0. `cleanup/collision.exit` is 2. `cleanup/clean.exit` is 0. `cleanup/collision-after-clean.exit` is 2. `cleanup/clean-again.exit` is 0. Stop and clean stderr files are empty. No prune. The private clone and the preoverlay archive directory were removed. `cleanup/owned-tmp.txt` records those paths.

`linux/foreign-before.txt` listed `oo-a308-pg`, `oo-a318-pg`, and `oo-a300-pg` before this review's builder started. `cleanup/foreign-after.txt` is empty. `cleanup/stop.stdout` names only `c067edebf700`. `cleanup/clean.stdout` names only the three `oo-a322-eef831aa-*` volumes. The stop and clean filters require `oo.owner=a322`, this run id, and fixed source `25dfc902`. This cleanup did not name the other owners.
