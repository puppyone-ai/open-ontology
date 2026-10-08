# Handoff

Owner of this directory: session `eef831aa-dba4-4af2-a78b-541bc9e0a376`. Verdict on repair `be4b596481d83435df2ede8abd9ebb2a8647c706`: **APPROVE_LIMITED**. Parent SP14 and E51 stay open. This directory does not accept them and does not change a ledger count.

## Result to carry

`install()` composes in both orders when both role pairs already exist and the sibling `oo_dict` matches the pin. Migration bytes stay `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` and `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd`. Replay accepts a `search_path` that merely contains `search_path=pg_catalog`, including `{search_path=pg_catalog, public}`. Raw `psql` of both files on one database still errors at `CREATE SCHEMA oo_dict` (`42P06`). Autocommit leaves `oo_enum_control` in place and creates the calendar view as `oo_calendar_admin` because `SET LOCAL` is warning `25P01`. That is unsupported. It is not a general fix.

`proof/proof.exit` is 0. `proof/oracle.log` is `oracle_ok` with 137 fact lines.

## Do not rerun

`linux/run-proof.sh` creates the containers and repeats the green cargo tests. `linux/continue-proof.sh` is the deny, raw `psql`, clippy, and oracle resume. Clippy and the green phases are already exit 0. Running either script again repeats work or collides with names that cleanup removes. `linux/resume` scripts from the a365 packet are not this session's drivers.

## Cleanup

`linux/cleanup.sh` removes a container, volume, or network only when `oo.owner=a376` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a376`. Named objects are `oo-a376-pg`, `oo-a376-proof`, `oo-a376-hash`, volumes `oo-a376-eef831aa-cargo`, `oo-a376-eef831aa-target`, `oo-a376-eef831aa-pgdata`, and network `oo-a376-eef831aa-net`. It also removes `/tmp/oo-a376-eef831aa-dba4-4af2-a78b-541bc9e0a376`. It does not prune and it does not remove other owners. `cleanup-done.txt` is `done`. `oo-a376-proof`, `oo-a376-pg`, the three volumes, the network, and that tmp directory were removed. `oo-a373-pg`, `oo-a375-pg`, `oo-fe01-rust-m6`, and `oo-fe01-pg-m6` were still running.

## Hash

After `cleanup-done.txt` is `done`, `linux/hash-tree.sh` runs twice inside the pinned Rust image, network none, one builder, 6 GiB, 1 CPU, `--pull never`. The walker is `linux/hash-oracle/main.rs`. It checks empty = `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` and `abc` = `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`, skips `SHA256SUMS` and `SHA256SUMS.sha256`, and writes `SHA256SUMS.sha256` as the digest of the sums body. Pass 2 includes the finished pass 1 logs. Pass 2's own console is copied in after that walk. Host `shasum` and Python are not the authority.

## Left open

SP14, E51, SP14-W04, SEM-D03, SEM-D04, and `enum_calendar_same_database`. The product source at `be4b596481d83435df2ede8abd9ebb2a8647c706` stays the reviewed repair. Later commits through report HEAD `0c3caa184c93bcd0af91c5d2aa75bc4ee96416d8` do not change `tools/calendar-store` or `tools/enum-store`. The oracle ran at `55be6e88e191e89748c8e9e9905f074bda87e810`.

No production call, no customer call, no foreign database, no push, no release.
