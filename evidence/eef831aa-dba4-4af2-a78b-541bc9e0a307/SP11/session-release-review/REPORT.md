# Session-release independent review

Verdict: **APPROVE_LIMITED** for the local-fixture orchestrator in `tools/session-release` at `72dc9e2db841de4de368216bbbe13d738f850ea9`, crate tree `2a408d25eca5a076b1f6295b7ce40b8797f70526`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a307`. The author record is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a295/SP11/session-release`. That same-session four-pass review is not this approval. No product file, root lock, connector manifest, spec, or ledger was edited. This file does not accept parent RT-D04, does not close SP11, and does not certify a physical release. `physical_release_certified` stays false. Containment is `local_fixture`. Exit 0 leaves the session `releasing`.

`AGENTS.md` is not in the repository. The release list in `docs/09-sandbox-and-agent-runtime.md` section 8 and the open RT-D04 row in `specs/open-ontology-v0.1-plan.md` were read with `tools/session-release/CONTRACT.md`, the author REPORT, FINDINGS, and HANDOFF, and the store and reducer trees named below.

## Archive

`linux/preoverlay` used `git archive 72dc9e2db841de4de368216bbbe13d738f850ea9` of `Cargo.lock`, `tools/session-release`, `tools/session-store`, and `tools/session-transition`. Repo HEAD at launch was `e8660132793b09ed1239a8c849f4f9498a6f17fc` and was not the build input. `72dc9e2` is the evidence commit whose parent is the author source commit `524c6f5197a0e31e8d9e8f3e06510c7ee3d1d823`. `git diff` of those four paths between the two commits is empty.

| Tree | At `72dc9e2` | Reviewed identity |
|---|---|---|
| session-release | `2a408d25eca5a076b1f6295b7ce40b8797f70526` | author crate tree |
| session-store | `f55ad766e225ae8f0d7a3913e10c681986efaca9` | same tree as `e213ce37140090e843ba98c188009f920962937d`, a281 APPROVE_LIMITED |
| session-transition | `a25733870f1ffac55c057dd399270905ccca9151` | same tree as `b0f7da4771e3c3625a45329c721e174f4380571c`, a272 APPROVE_LIMITED |

This review does not re-approve the store or the reducer. It checks that the archive contains those trees and that `drive.rs` only sends `Event::Cancel` and `Event::RecordReleaseStep`. It has no `CompleteRelease` constructor. `complete_release_called` is set true only in the unit test that expects success to stay false.

## Recipe

Origins and hashes are in `recipe-origin.txt`. `pg/pg-wrap.sh` is a byte copy of the a200 wrapper (`25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f`). `tools/sha256_manifest.rs` is a byte copy of the a204 manifest tool (`d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`).

Minimal changes from the a295 impl-5 launch: the archive already contains `tools/session-release`, so there is no working-tree overlay; the network, volumes, database, and CA are `a307`; memory is 6 GiB and CPUs are 1; the author tests still read `OO_A295_FIXTURE`, pointed at this session's fixture directory.

## PostgreSQL

One private server, container `oo-a307-pg`, id `ee87546724a7af21ef108210878cbc32e2e98d92679988db4b295742344902b7`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/up.exit` is 0. `pg/ports.json` is `{}`. Network `oo-a307-eef831aa-net`, bridge, internal false. CapDrop `ALL`, CapAdd `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Privileged false. Certificate subject `CN=oo-a307-pg` with SAN `DNS:oo-a307-pg`. Passwords and keys stayed under `/tmp/oo-a307-eef831aa-dba4-4af2-a78b-541bc9e0a307`. A fixed-string scan of this evidence directory against those password files reported `leak=0`.

## Decisive run

`linux/preoverlay`, container `oo-a307-preoverlay`, id `39723a3ca4e11034c503bd995ef4702740f8734ae290113e423271f06b0cc60d`. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a307-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`. `capacity.txt` recorded `own=0 global=0 host_cargo=0 own_postgres=1 cap_global=10 cap_own=1`. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

| File | Exit |
|---|---|
| `docker-run.exit` | 0 |
| `signal-probe.exit` | 0 |
| `archive.exit` | 0 |
| `version.exit` | 0 |
| `lock-triples-build.exit` | 0 |
| `lock-triples.exit` | 0 |
| `clippy-component.exit` | 0 |
| `fetch.exit` | 0 |
| `test.exit` | 0 |
| `clippy.exit` | 0 |
| `unchanged.exit` | 0 |

`lock-triples.stdout`: `summary release_registry=128 store_registry=128 reducer_registry=11 root_registry=373 missing=0 mismatch=0 direct_pins_present=true`. Direct pins are serde 1.0.229, serde_json 1.0.151, tokio 1.53.1, and tokio-postgres 0.7.18. The oracle is `tools/lock_triples.rs`, compiled with `rustc --edition 2024`. It is not a crate test. No `cargo update` and no `cargo generate-lockfile` ran. `unchanged.exit` 0 means every archived file was still byte-identical after the tests.

The empty private cargo volume made the first offline `cargo test` exit 101: `no matching package named serde_json` while offline. That attempt is `test-offline-attempt.exit` 101. `cargo fetch --locked` then exited 0, and the retry exited 0. Clippy was `cargo clippy --locked --offline --all-targets -- -D warnings` and finished in 53.18s.

Final test stdout:

- library: `ok. 9 passed; 0 failed` in 0.00s
- binary: `ok. 0 passed; 0 failed`
- live: `ok. 9 passed; 0 failed` in 15.16s
- lock subset: `ok. 1 passed; 0 failed`

Live names: `blocked_query_and_slow_child_release_without_closing`, `cancel_cannot_treat_an_idle_transaction_as_stopped`, `cancel_role_is_limited_and_runtime_cannot_create`, `escaped_grandchild_and_canary_are_not_confirmed`, `inaccessible_postgres_does_not_spawn_or_close`, `missing_workspace_and_foreign_path_have_no_side_effects`, `noisy_transcript_is_not_confirmed_and_workspace_remains`, `restart_after_the_orchestrator_is_killed_does_not_signal_or_certify`, `revision_conflict_and_event_id_conflict_do_not_close`.

The signal probe printed `group_stopped=19` and exited 0 inside the cap-dropped container. `python3_present=1` in the image. No Python process hashed a lock, a row, or an expected value.

`FROZEN.md` was written before `tools/lock_triples.rs` and before the fixture. No product test was added.
