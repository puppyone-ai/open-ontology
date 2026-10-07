# Handoff

Owner of this directory: session `eef831aa-dba4-4af2-a78b-541bc9e0a365`. Parent SP14 and E51 stay open. This directory does not accept them.

## Result to carry

`install()` on one control database works in both orders when both role pairs already exist and the existing `oo_dict` matches the sibling pin. The accepted migration bytes stay `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` and `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd`. Raw `psql` of both files on one database still errors at `CREATE SCHEMA oo_dict`. A non-owner must call `dict_grant_runtime`; a direct `GRANT USAGE` does not change `nspacl`.

`proof/fixed-commit.txt` from fetch attempt 1 contained only `9d846541cf2e2d6c7d43a518bb7b9ad8e8368d08`. That value is not this packet's commit. The file now records the pre-commit HEAD and that attempt-1 line. `linux/pre-edit-smoke/fixed-commit.txt` remains the smoke base `2177b43f1624e9680f61c6b1327450956bec82e5`.

## Do not rerun

`linux/resume-proof-6.sh` drops and recreates `oo_a365_cal_first`, then runs the green phases. The exits under `proof/` outside the attempt directories are already 0. `linux/resume-proof-4.sh` and `linux/resume-proof-5.sh` exit 2 immediately. `linux/run-proof.sh` is attempt 1. `linux/continue-proof.sh` is the fetch and lib run plus the first solo failure.

## Cleanup

`linux/cleanup.sh` removes a container, volume, or network only when `oo.owner=a365` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a365`. Named objects are `oo-a365-pg`, `oo-a365-proof`, volumes `oo-a365-eef831aa-cargo`, `oo-a365-eef831aa-target`, `oo-a365-eef831aa-pgdata`, and network `oo-a365-eef831aa-net`. It also removes `/tmp/oo-a365-eef831aa-dba4-4af2-a78b-541bc9e0a365`. It does not prune and it does not remove other owners. `oo-a370-builder` was running at packet time and is outside this label pair. `cleanup-done.txt` is the completion record. `cleanup-targets.txt` and `cleanup-kept.txt` are written by that script.

## Hash

After `cleanup-done.txt` is `done`, `linux/hash-tree.sh` runs twice inside the pinned Rust image, network none, one builder, 6 GiB, 1 CPU. A walk taken while the reports still named `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9` was discarded after HEAD advanced to `6283bc16f6d408ffa38dbbf8837cc4f80268bbcb`. The passes below are the walk of this tree. The walker is `linux/hash-oracle/main.rs`, a std-only copy of `sha256`, `hex`, and `sha256_hex` from `tools/enum-store/src/sha256.rs`. The product file is not edited. The program checks empty = `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` and `abc` = `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` before it walks. Host `shasum` and Python are not the authority for this tree.

Each pass writes `linux/hash-pass-N/capacity.txt` before the walk and copies `console.log`, `docker-run.exit`, `hostconfig.txt`, and `container-removed.txt` after the container exits. The walk skips `SHA256SUMS` and `SHA256SUMS.sha256`. Pass 2 therefore includes the finished pass 1 logs. Pass 2's own console and exit are copied in after that walk. `SHA256SUMS.sha256` is the digest of the sums body.

## Left open for the parent

SP14, E51, SP14-W04, SEM-D03, SEM-D04, and the parent family list that still names `enum_calendar_same_database`. This packet proves the Rust `install()` path. It does not rewrite the SQL files, and it does not edit a parent ledger.

No production call, no customer call, no foreign database, no push, no release.
