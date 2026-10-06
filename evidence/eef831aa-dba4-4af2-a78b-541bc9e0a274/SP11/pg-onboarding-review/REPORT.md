# pg-onboarding independent review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a274`. Tool commit `50d4e705a2bb4ba014919ef1e071da065cf8e9b7`. Fixed integrated archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`. The tool tree at both commits is `b0e6fd768c8d919df3b8d6683dd44d444608c78c`. This report covers that frozen tool and this evidence directory. It does not accept a parent unit, M1-I01, M1-I02, semantic publication, authentication, natural language, or a release.

## Result

The live proof status is `ready_for_trusted_query`.

Inspect sealed a captured draft and stopped at `awaiting_review`. `source_cut` is 48, `target_fence` is 1, and `mapping_run_generation` is 48. The config identity is `a4a889fba1436935fb1fe1af9c30a8a79109794d94600fa4ce9d2e1b21f7b618`, recomputed from the non-secret import fields. Apply ran the reviewed compilers built from the archive and a per-role `query-pg` smoke. `analyze_published` is false. `estimate_is_exact` is false. `database_rollback` is false. `database_release` is false. Success `database_state` is `child_compilers_applied_no_release`.

Clerk provision effect is `installed`. Peek provision effect is `replay`. Both column-ACL effects are `installed`. Grantor is `oo_fixture_admin`. Hidden column `shop.customer.private_note` has reason `classification.absent`. Clerk order amounts are the int8 extremes and zero, unchanged. Comment marker `甲注评` is absent from the catalog, the captured draft, and both outcome files.

The proof oracle is the frozen `tools/pg-onboarding/tests/live_chain.rs` from the archive. `linux/chain/live.exit` is 0. `linux/chain/chain.exit` is 0. `live/work/oracle-ok.txt` is `ready_for_trusted_query`.

## Repair checks without a second PostgreSQL

`linux/preoverlay-attempt-3` ran the frozen suite with `live_chain` ignored. `test-count.txt` is `passed=24 failed=0 ignored=1`. That suite includes the actual CLI child fixture `cli_failed_child_keeps_marker_and_static_stderr`, the duplicate root and nested refusals, and the shared visitor test `json_doc::tests::duplicate_root_nested_and_escaped_keys_refuse`. No PostgreSQL was running for that builder.

`linux/focused` repeated those 24 tests and ran Clippy `--locked --offline --all-targets -- -D warnings`. `focused.exit` is 0. `clippy.exit` is 0. No review overlay file was added.

## Builders

Linux arm64 Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Toolchain text `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Each new builder used 6 GiB and 1 CPU, with every capability dropped. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 17.10, 1 GiB, 1 CPU. One private database at a time.

| Run | Exit | Record |
| --- | --- | --- |
| preoverlay | 1 | Tests were 24 passed, 1 ignored. The name check looked up `tests::duplicate_root_nested_and_escaped_keys_refuse`. Cargo printed `json_doc::tests::duplicate_root_nested_and_escaped_keys_refuse`. |
| preoverlay-attempt-2 | 125 | Docker did not see the archive directory after it was removed and extracted again. `DIAGNOSIS.txt`. |
| preoverlay-attempt-3 | 0 | Corrected name. Same frozen bytes. Container `8e3640d67580a4628f32398e6ad05b975b720cad760b590fec4f620b4f0ab2f1`. |
| focused | 0 | 24 passed, live ignored, Clippy clean. Container `b20e1551f59130517d78fc4ba6f8af3c9421608a847346804c2321083528c0d0`. |
| chain | 0 | Fresh cluster. Live oracle passed in 144.89s. Container `bc40715af21b26efb3b3ccda9492b31a74cf3bc0e41542ebe083fa89dfdc6d01`. |

No `cargo update`. No product crate edit. No root manifest or root lock edit. a207 remains the only connector manifest and root-lock writer.

## Inventory

`linux/manifest/manifest.exit` is 0 and is not a completed inventory. The walker binary was placed on a container tmpfs that refused execution. `linux/manifest/DIAGNOSIS.txt` records that. The successor command is `linux/manifest-attempt-2/launch.sh`. It compiles the same walker onto `/out`. This report was written before that command's console existed.

## Live fixture

Private network `oo-a274-eef831aa-net`. Container `oo-a274-pg`, id `c55131f2af18faa15ddbd14e8155f0ac0bd3348064f0ba6c5f89e422f123b4e3`. Network alias `oo-a261-pg`, which is the hostname in the frozen oracle. Databases `oo_a261_src` and `oo_a261_dst`. Schema `shop`, tables `customer` and `order_doc`. Roles `oo_a261_reader`, `oo_a261_runtime`, and shared login `oo_a261_login` for slugs `clerk` and `peek`. Autovacuum was off. TLS used a new private CA whose subject is `oo-a274-ca`. The server certificate SAN is `DNS:oo-a261-pg`. Passwords stayed in files under `/tmp/oo-a274-eef831aa-dba4-4af2-a78b-541bc9e0a274/secrets` and were not placed on argv. No password file or private key is in this evidence directory.

The host watcher granted `oo_pipeline_runtime` to the runtime login after init created that role, then wrote the ready file. `grant.out` is `granted`. The onboarding CLI does not issue that grant. It waits for the ready file. That grant is fixture configuration. It is not authentication and it is not a business-column grant.

Query exits: `clerk-before` 5 with `query-pg: policy`; `clerk-hidden`, `clerk-insert`, and `clerk-pos` 6 with `query-pg: guard`; the five positive reads exit 0.

## Caller

The config field `caller` is `trusted_local_admin`. This is an operator config on a local machine. It is not a public authentication check.
