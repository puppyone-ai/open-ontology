# AGENTS.md

Harbor Queue is a small job queue library with a Rust core, Python tools and a TypeScript dashboard.

## Setup

- Install the toolchain with rustup, then run pnpm install inside web/.
- The Python tools need Python 3.11.

## Commands

```bash
cargo build --workspace
cargo test -p harborq-core
cargo fmt --all
pnpm --dir web test
python3 tools/report.py --since 7d
```

## Rules

- Never edit files under core/src/generated/ by hand; run make generate instead.
- Run the unit tests with cargo test -p harborq-core before every commit.
- Keep public functions documented with a doc comment.
- Do not add a dependency without an ADR in docs/adr/.
- Use cargo fmt --all before committing Rust code; CI rejects unformatted code.
- Prefer small pull requests that change one thing.
- Web code must pass pnpm --dir web test and eslint.

## Architecture notes

- The queue stores jobs in SQLite (see docs/adr/0001-use-sqlite-for-storage.md).
- Workers poll core/src/worker.rs every 200 ms.
- The dashboard in web/ reads the same database through a read-only API.

Owner: Platform Team
