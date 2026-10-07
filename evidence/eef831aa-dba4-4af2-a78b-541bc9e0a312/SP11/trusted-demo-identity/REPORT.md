# OPS-D01 trusted-demo identity

Session `eef831aa-dba4-4af2-a78b-541bc9e0a312`. Fixed product archive `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`. This child parametrizes the public Docker demo's resource identity. It is not a new unit or component.

The same author wrote the code, ran the proof, and wrote this report. That is not an independent review. Independent fixed review is still required. This result does not accept the parent SP11-W06, M1, I01, I02, a release, authentication, authorization, semantic publication, natural language, a model provider, a harness, Compose, or an enterprise endpoint. No enterprise credential was sought.

## Result

`start.sh --manifest operator-manifest.json --public-fixture` exited 0. Host log `linux/public-fixture.host.log` runs from `2026-10-07T10:34:31Z` to `2026-10-07T10:43:17Z`. PostgreSQL came ready, the fixture loaded, the volume restarted, and the fixture state was `present`.

Receipt `live/receipt.json`, protocol `oo.deploy.trusted-demo-receipt/v1`, status `ready_for_trusted_query`. Fields: `project=open-ontology`, `owner=a312`, `run_id=eef831aa-dba4-4af2-a78b-541bc9e0a312`, `source_id=public_shop`, `fixed_commit=25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`. Non-claims in that file: `human_approval=not_claimed`, `authentication=not_claimed`, `authorization=not_claimed`, `semantic_publication=missing`, `natural_language=unsupported`, `model_provider=unsupported`, `harness=not_performed`, `m1_i01=not_accepted`, `m1_i02=not_accepted`, `parent_acceptance=false`.

`config_identity_sha256=34a6170dd3e933997471b0ed4e8a0efbbe828ba77cccda3164157467a62d5711`. `schema_hash=da877f8bc78b7a9a14b8648004bfd904d8bc89b774bbca1253c1758927a74a2f`. Role packs: north `48d111586df69a765d9917699b15fe9a9c1d6e71e121989bcbd995b7a0207239`, narrow `58d5a9e5b4f58554ec0b800b3f2cd92d08d768a7dab4bb07330bdb5cb71b2080`.

North query `live/run/steps/query-north-customer.stdout` returned three rows and exit 0: customer 41 `丙号` active, customer 42 null inactive, customer 43 `丁铺` active. Summary `returned_rows=3`, `original_sha256=e617988aaadeab6eb1c9302a033a83c2b3ee7df6133fc005b0d1de9079837400`. The restart smoke wrote the same three rows and the same original hash to `live/smoke/north-customer.stdout`. `live/smoke/smoke-ok.txt` says `role_pack_unchanged` and `query_rows_repeated`.

`query-north-hidden.exit` is 6 and stderr is `query-pg: guard`. `query-north-insert.exit` is 6 and stderr is `query-pg: guard`. `query-north-before.exit` is 5 and stderr is `query-pg: policy`. The bad review `live/bad-run/apply-outcome.json` is `status=failed`, `step=reviewed-effective-oos`, `detail=exit`. The child stderr contains `decision.unknown`. Column ACL, mapping reader, analyze, role pack, and trusted query files were not written in that bad run. The good apply then reached `ready_for_trusted_query`.

Secret scans: `secret-scan.txt` is `secret_absent files=318`. `smoke-secret-scan.txt` is `secret_absent files=349`.

## Chain

Reviewed CLI binaries were built from a `git archive` of `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480` mounted at `/src`. `linux/public/cargo-update-invoked.txt` is `no`. The driver and the shell scripts came from `deploy/trusted-demo` mounted at `/deploy`. Driver source SHA-256 is `60f2ba592e456418e6da94c90e0a4815aa69299bc62adc74046ed07432b9aef1` (`linux/hash-source/source-sha256.txt`, path `./src/drive.rs`). Driver artifact SHA-256 is `0e0e3a8c90c9d67526c35cd720d7daab30fa69619d04676da4ca2167e23f051c` (`linux/public/bin-sha256.txt`, `/cargo-home/bin/drive`). `tools/sha256_manifest.rs` remains `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`.

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`. Container `oo-a312-pg` id `902b13fdf96f327549a143e92166f5ffe235bc934b574c2c065318b3ace604dc`. `pg/server.txt` records `server_version=17.10`, `autovacuum=off`, `ssl=true`. `pg/ports.json` is `{}`. `pg/restart-state.txt` is `restart_state=present`.

There is no default UUID. The operator file is `operator-manifest.json`. The durable copy is `deployment.json`. Owner `a312` is the last four characters of the run id. Docker labels on the owned objects were `oo.project=open-ontology`, `oo.owner=a312`, `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a312`, and `oo.fixed=25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`. Inspect config `owner` is the run id UUID, because that tool requires a UUID. The historical `a305` manifest remains valid only when it is passed explicitly. This run did not pass it and did not create an `a305` object.

## Builders and smoke

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, toolchain text `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Each new builder was 6 GiB (`Memory=6442450944`), 1 CPU (`NanoCpus=1000000000`), `Privileged=false`, `CapDrop=["ALL"]`, `OOMKilled=false`. One builder at a time. `linux/public-fixture.capacity.txt` was `own=0 global=0 host_cargo=0 own_pg=0`.

Onboarding test count `linux/public/test-count.txt` is `passed=24 failed=0 ignored=1`. All 33 files in `linux/public/phases/` are 0. All 7 files in `linux/smoke/phases/` are 0. The in-chain driver tests passed 7, and driver Clippy `-D warnings` exited 0 before the workspace binaries.

| Run | docker-run.exit | Record |
| --- | --- | --- |
| `linux/pre-edit-smoke` | 127 | `bash -lc` cleared `PATH`, so `rustc` was not found. Preserved. No product edit in that attempt. |
| `linux/pre-edit-smoke-attempt-1` | 0 | Pre-edit driver tests: 5 passed. `bash -c` and the image `PATH`. |
| `linux/post-edit-drive-attempt-1` | 1 | `historical_manifest` was marked `#[test]` and returned `&str` (`E0277`). Preserved. No product network or PostgreSQL. |
| `linux/post-edit-drive-attempt-2` | 0 | 7 driver tests passed. Clippy `-D warnings` exited 0. Container and `oo-a312-postedit-net` removed. |
| `linux/public` | 0 | Chain reached `ready_for_trusted_query`. Container `d251f0c379f7222167eb9852501f44f6b46f4dedd0547f62e76edd585b0a050c`, removed after the run. |
| `linux/smoke` | 0 | Restart smoke. Container `b24ebe05997e527120b535414855a41161e0e395c4f1d60c690b2e13a830cc85`, removed after the run. |

Host `shasum` and Python were not used as oracles. Source inventory `linux/hash-source/source-sha256.txt` has `files=23`, with the empty and `abc` self-checks in `linux/hash-source/console.log`. An earlier walk of the tree before the README wording fix is preserved at `linux/hash-source-attempt-1/`.

## Inventory

`SHA256SUMS` is the Safe Rust walk of this evidence directory after this report, `FINDINGS.md`, and `HANDOFF.md`. The walker is `deploy/trusted-demo/tools/sha256_manifest.rs`. It skips its output path. One private builder, network `none`, 6 GiB, 1 CPU, compiles that file and runs it. `linux/hash-manifest/` is copied after that walk, so those logs are not members of `SHA256SUMS`. The container stdout, including `files=` and the self-checks, is that directory's `console.log`.

## Cleanup

Owned cleanup used `./deploy/trusted-demo/start.sh --manifest operator-manifest.json`. `cleanup/stop.exit` is 0. Stdout removed container `902b13fdf96f` and printed `stopped_owned_containers`. Volumes and the private tmp remained. `cleanup/collision.exit` is 2. Stderr refused the existing run directory, private tmp, network `oo-a312-eef831aa-net`, and volumes `pgdata`, `cargo`, and `target`. `cleanup/after-collision.txt` shows no `oo-a312-pg` and no `oo-a312-build`, and the three volume timestamps stayed `2026-10-07T10:34:32Z` and `2026-10-07T10:34:35Z`. `cleanup/clean.exit` is 0. Stdout removed the three volumes and printed `cleaned_owned_private_resources`. The network and `/tmp/oo-a312-eef831aa-dba4-4af2-a78b-541bc9e0a312` were removed. Stop and clean stderr files are empty. No prune. After that removal, `cleanup/collision-after-clean.exit` is 2 because `live/run` remains. `cleanup/clean-again.exit` is 0.

`oo-a300-pg` id `a68ce9caf5d6489211e27aaa5f5bbf75e75a883f110129679c76ecec7166bbd1` was running before the fixture and was still running after clean. `oo-a301-pg` and `oo-a310-pg` were recorded in `linux/foreign-pg-ids-during.txt` and were already absent before stop. This cleanup did not name them.
