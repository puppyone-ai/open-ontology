# Handoff

Owned paths:

- `tools/pg-onboarding/**` at commit `50d4e705a2bb4ba014919ef1e071da065cf8e9b7` (parent `efdffed87f4e19070536cbd6a72fa1a6583e639e`).
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a268/SP11/pg-onboarding-outcome-repair/**`.

Fixed product archive `56710cc73cfe1663272ecaa503984e1f663428ad`. Do not treat the parent commit or HEAD as that archive. `Cargo.toml` and `Cargo.lock` in the tool commit match that archive. a207 remains the only writer of the connector manifest and root lock. This repair did not edit them.

## What the next reader should open

- Contract: `tools/pg-onboarding/CONTRACT.md`. A copy is `source/repaired/CONTRACT.md`.
- Pre-repair blobs: `source/blobs/outcome.rs` and `source/blobs/spawn.rs`.
- Repaired blobs: `source/repaired/`.
- Pre-edit smoke of the fixed archive: `linux/smoke/`. `smoke.exit` is 0.
- Red compile failure: `linux/red/`.
- Red assertion runs: `linux/red-attempt-2/`, `linux/red-attempt-3/`, and `linux/red-attempt-4/`. Attempt 4 is the final red suite.
- Green tests and Clippy: `linux/green/`. `green.exit` is 0.
- Recipe origins: `origin/recipes.txt`.
- Tool file inventory from the Rust walker: `source/tool-sha256.txt`.
- Evidence inventory: `SHA256SUMS`.

The walker skips its output path. `SHA256SUMS` is copied into this directory after the walk, so it is not a member of the list. These files are also written after the walk and are absent from the list:

- `linux/manifest/console.log`
- `linux/manifest/hostconfig.txt`
- `linux/manifest/mounts.txt`
- `linux/manifest/tmpfs.txt`
- `linux/manifest/docker-run.exit`
- `linux/manifest/container-removed.txt`
- `cleanup.log`

## Commands the proof used

Smoke, before the repair was mounted: `linux/smoke/launch.sh`.

Final red, archive sources plus these tests: `linux/red-attempt-4/launch.sh`.

Green, working tree that was committed as `50d4e705a2bb4ba014919ef1e071da065cf8e9b7`: `linux/green/launch.sh`.

No live chain command was run.

## Left in place until cleanup

This session's resources are network `oo-a268-eef831aa-net`, cargo volume `oo-a268-eef831aa-cargo`, target volume `oo-a268-eef831aa-target`, and `/tmp/oo-a268-eef831aa-dba4-4af2-a78b-541bc9e0a268`. Builders are removed at the end of each launch. Cleanup is `cleanup.sh` in this directory. It removes only `oo.owner=a268` resources for this session. It does not prune and it does not touch other owners.

## Outside this repair

The ops console, natural-language surface, public authentication, semantic publication, and the full harness remain later work. A new expansion needs its own frozen contract before code. This directory does not accept the parent unit.
