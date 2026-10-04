# Commands

Fixed archive `4ed22b8c1189e72b06b20a45335860ac9a3ea44e`, tree `fafb0fe6752b8240eb2b29025fc38bccdfb7144d`. Smoke and both probe attempts compiled that archive. The green probe copied `overlay/target_installer_a152_review.rs` into the archive snapshot before `cargo test`. It did not edit the git tree.

Workspace head at smoke launch: `057e30dbb16857d6af889a2a5c23dde49dcde8a6`. Workspace head at the green probe: `f1fbda183b32bc959c9c8a794f358cc9ecdc8dac`. The archive identity did not follow that head.

Password transport was a file under `/tmp/oo-a152-eef831aa-dba4-4af2-a78b-541bc9e0a152/pg-secrets/`. The value was not an environment variable and is not copied here.

## Smoke

```text
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a152/SP04/target-installer-review/linux/smoke/launch.sh
```

Inside the container:

```text
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
```

| File | Exit |
|---|---|
| `linux/smoke/toolchain.exit` | 0 |
| `linux/smoke/snapshot.exit` | 0 |
| `linux/smoke/clippy-component.exit` | 0 |
| `linux/smoke/fetch.exit` | 0 |
| `linux/smoke/lib.exit` | 0 |
| `linux/smoke/clippy.exit` | 0 |
| `linux/smoke/docker-run.exit` | 0 |

Lib result: `ok. 9 passed; 0 failed`. Elapsed 37 seconds. Image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Container `f423def6076e66e1a991feb948addcd6bedd35eb71a344cc61d89451f8b32bb6`.

## PostgreSQL

```text
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a152/SP04/target-installer-review/pg/launch.sh
```

`pg/ready.txt` is 1. `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Container `ad308b26ab64b41f23e37d15eb8e163afd7d204231c589825493a8aa28b0591c`. Privileged false. Published ports empty. Cap drop ALL, with CHOWN, DAC_OVERRIDE, FOWNER, SETGID, and SETUID added so the image entrypoint can start.

## Probe attempt 1

```text
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a152/SP04/target-installer-review/linux/probe/launch.sh
```

| File | Exit |
|---|---|
| `linux/probe/attempt-1/toolchain.exit` | 0 |
| `linux/probe/attempt-1/snapshot.exit` | 0 |
| `linux/probe/attempt-1/clippy-component.exit` | 2 |
| `linux/probe/attempt-1/docker-run.exit` | 2 |

Container `e9cde77ef46c107b987eed6d077c79874e689230bb30b9d96ff947a4e0b86d5c`. The oracle did not run. The exited container was removed before the retry. The PostgreSQL container was left up.

## Probe

Same launch script after the Clippy presence check stopped failing the oracle. Smoke had already run Clippy.

```text
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test target_installer_a152_review -- --include-ignored --nocapture --test-threads=1
```

| File | Exit |
|---|---|
| `linux/probe/toolchain.exit` | 0 |
| `linux/probe/snapshot.exit` | 0 |
| `linux/probe/clippy-component.exit` | 0 |
| `linux/probe/review.exit` | 0 |
| `linux/probe/docker-run.exit` | 0 |

Result: `ok. 2 passed; 0 failed` in 15.75 seconds. Elapsed 19 seconds. Container `ba4148c94ab6b53bed17df77bfe4798f4417af39c759d5eff273b75d8fc8de41`. Outcome: `linux/probe/live/outcome.txt`, last line `installer_review_ok`.

## Identity checks on the host

```text
git rev-parse 4ed22b8c1189e72b06b20a45335860ac9a3ea44e^{tree}
git diff --stat 6cd955af38ce426bb1102a6e078e8580febed776 4ed22b8c1189e72b06b20a45335860ac9a3ea44e -- crates/oo-pipeline/migrations/0000_bootstrap_roles.sql crates/oo-pipeline/migrations/0001_target_fence_expand.sql crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql
git diff --stat b5415544c9eb8d608eafd411884627dda9ee7c84 4ed22b8c1189e72b06b20a45335860ac9a3ea44e -- crates/oo-pipeline/migrations/0004_physical_mapping.sql crates/oo-pipeline/src/physical_mapping.rs
```

Tree matched `fafb0fe6752b8240eb2b29025fc38bccdfb7144d`. Both diffs were empty.
