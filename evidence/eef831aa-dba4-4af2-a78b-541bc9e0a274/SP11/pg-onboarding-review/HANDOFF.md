# Handoff

Owned path:

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/**`

No product path was edited. The reviewed tool remains `tools/pg-onboarding/**` at commit `50d4e705a2bb4ba014919ef1e071da065cf8e9b7`. Fixed product archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`. Do not treat HEAD as a different archive. a207 remains the only writer of the connector manifest and root lock.

## What the next reader should open

- Current contract: `tools/pg-onboarding/CONTRACT.md`. A sealed copy is `source/blobs/CONTRACT.md`. The historical a261 contract was not edited.
- Author reports: a261 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a261/SP11/pg-onboarding-cli/` and a268 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a268/SP11/pg-onboarding-outcome-repair/`.
- Recipe origins and hashes: `origin/recipes.txt`.
- Passing pre-overlay smoke: `linux/preoverlay-attempt-3/`. `smoke.exit` is 0.
- Failed attempts stay in `linux/preoverlay/` and `linux/preoverlay-attempt-2/`.
- Passing focused tests and Clippy: `linux/focused/`. `focused.exit` is 0.
- Passing live proof: `linux/chain/` and `live/run/apply-outcome.json`.
- Tool bytes compared with the archive: `source/blobs/` and `source/tools/pg-onboarding/`.
- Evidence inventory: `SHA256SUMS`, produced by `tools/sha256_manifest.rs` in `linux/manifest-attempt-2/`.
- Failed inventory attempt: `linux/manifest/`. Its `manifest.exit` file is 0 and is not a completed walk. See `linux/manifest/DIAGNOSIS.txt`.

The walker skips its output path. `SHA256SUMS` is copied into this directory after the walk, so it is not a member of the list. The attempt-2 console is written under `/tmp/oo-a274-eef831aa-dba4-4af2-a78b-541bc9e0a274/manifest-out-2/console.log` while the walker runs, then copied into the evidence tree after the container exits. `manifest.exit` is written by the container after the walker returns. These files are absent from the list:

- `linux/manifest-attempt-2/console.log`
- `linux/manifest-attempt-2/hostconfig.txt`
- `linux/manifest-attempt-2/mounts.txt`
- `linux/manifest-attempt-2/docker-run.exit`
- `linux/manifest-attempt-2/manifest.exit`
- `linux/manifest-attempt-2/container-removed.txt`
- `cleanup.log`

## Commands the proof used

Pre-overlay, before any review file was added to the tool tree: `linux/preoverlay-attempt-3/launch.sh`.

Focused tests and Clippy, same frozen bytes: `linux/focused/launch.sh`.

Live chain, after that exit file is 0: `linux/chain/launch.sh`. It builds the pinned compilers from archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb` and runs `cargo test --test live_chain -- --ignored`. One private PostgreSQL and one 6 GiB builder. No pull, no prune, no host network, no docker socket mount.

## Left in place until cleanup

The passing chain leaves `oo-a274-pg`, volume `oo-a274-eef831aa-pgdata`, network `oo-a274-eef831aa-net`, cargo volume `oo-a274-eef831aa-cargo`, target volume `oo-a274-eef831aa-target`, and `/tmp/oo-a274-eef831aa-dba4-4af2-a78b-541bc9e0a274`. Cleanup is `cleanup.sh` in this directory. It removes only `oo.owner=a274` resources for this session. It does not prune and it does not touch other owners.

## Outside this review

The ops console, Compose parent, natural-language surface, public authentication, semantic publication, and the full harness remain later work. A new expansion needs its own frozen contract before code. This directory does not accept the parent unit.
