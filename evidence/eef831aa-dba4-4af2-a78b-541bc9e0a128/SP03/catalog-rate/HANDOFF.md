# HANDOFF — bounded catalog reads

Independent review starts at this commit. This session is not approval and does not close an R1 checkbox.

| | |
| --- | --- |
| Commit | `ccd93e1adc34b24ab998d90972549f8cffb59f0d` |
| Tree | `ef43877346afadcdccaf26fe6982c6b4e8b65902` |
| Parent | `182702e6e67d16de10f280eaf422fce4e2f5a5f2` |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a128` |

`git show --stat ccd93e1adc34b24ab998d90972549f8cffb59f0d` is the eight owned paths only. Evidence under this directory is a later commit.

## Read first

1. `FINDINGS.md` — the rate repair and the retained failures.
2. `REPORT.md` — image, toolchain, commands, and exits.
3. `verify/out/console.log` — lib 55, `catalog_rate_live`, unmodified `snapshot_live`, clippy 0.
4. `verify/out/postgresql.log` — combined reader log. Secret hits 0.
5. a125 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a125/SP03/current-schema-guard/live/out` — the pre-edit `snapshot_live` exit 101. Left as it was.

## Review bounds

Build with Rust 1.94.0, `--locked --offline`, from `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. PostgreSQL 17.10. The passing live order is `catalog_rate_live` then unmodified `snapshot_live` on one freshly initialized snap database and one log.

Still open: all 87 goals, supply, customer release, R2, R3, R4, and any R1 checkbox. Portal, Arrow, import, and CLI work stay parallel. This handoff does not close them.

## Runtime left behind

`cleanup.txt` records `cleanup_start=2026-10-03T12:03:34Z`. The three owned containers and network `eef831aa-dba4-4af2-a78b-541bc9e0a128-net` were removed. Password files under `/tmp/oo-a128-eef831aa-dba4-4af2-a78b-541bc9e0a128/secrets` were removed after the evidence logs scanned at 0 hits. Owned volumes were left in place and were not pruned:

- `eef831aa-dba4-4af2-a78b-541bc9e0a128-cargo`
- `eef831aa-dba4-4af2-a78b-541bc9e0a128-target`
- `eef831aa-dba4-4af2-a78b-541bc9e0a128-pglog`

a125 volumes `eef831aa-dba4-4af2-a78b-541bc9e0a125-cargo`, `-target`, and `-pglog` are outside this session. a127 containers and volumes are outside this session.
