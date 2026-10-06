# Handoff

Owned paths:

- `tools/pg-onboarding/**` at commit `1b314d73d5a105898921e453ef0382fdad96d069` (parent tool commit `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`).
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a261/SP11/pg-onboarding-cli/**`.

Product archive used for compiler binaries: `1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368`. Do not treat HEAD as that archive.

## What the next reader should open

- Contract: `contract/pg-onboarding-cli-v1.md`.
- Passing focused tests and Clippy: `linux/postedit-attempt-4/`.
- Passing live proof: `linux/chain-attempt-2/` and `live/attempt-2/run/apply-outcome.json`.
- Failed attempts stay in `linux/smoke`, `linux/postedit`, `linux/postedit-attempt-2`, `linux/postedit-attempt-3`, `linux/chain`, and `live/run`.
- Recipe origins: `origin/recipes.txt`.
- File inventory: `SHA256SUMS`. The manifest tool skips its own output, and the console, exit file, and copied slice files under `linux/manifest/` are written after the walk, so those four names are absent from the list.

## Commands the proof used

Post-edit, from a clean capacity check: `linux/postedit-attempt-4/launch.sh`.

Live chain, after that exit file is 0: `linux/chain-attempt-2/launch.sh`. It rebuilds the pinned compilers from the fixed archive and runs `cargo test --test live_chain -- --ignored`. One private PostgreSQL and one 6 GiB builder. No pull, no prune, no host network, no docker socket mount.

## Left in place until cleanup

The passing chain leaves `oo-a261-pg`, volume `oo-a261-eef831aa-pgdata`, network `oo-a261-eef831aa-net`, cargo volume `oo-a261-eef831aa-cargo`, target volume `oo-a261-eef831aa-target`, and `/tmp/oo-a261-eef831aa-dba4-4af2-a78b-541bc9e0a261`. Cleanup is `cleanup.sh` in this directory. It removes only `oo.owner=a261` resources for this session. Run it after this evidence is saved. It does not prune and it does not touch other owners.

## Outside this unit

The ops console, Compose parent, natural-language surface, public authentication, semantic publication, and the full harness remain later work. A new expansion needs its own frozen contract before code.
