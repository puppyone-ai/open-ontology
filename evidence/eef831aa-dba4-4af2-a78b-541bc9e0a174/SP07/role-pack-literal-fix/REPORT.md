# a174 role Pack literal and name closure repair

Source commit `eff1959bf028a65b20b7fca855d79c23e4b188fd`, tree `2f08d4ecef7ad2f3b617e8a04afcb45f0cf0186f`, parent `9e3f757fb34746c64c6fca3d60cf0c5e342a1885`.

Review `95289c50` requested changes on `ef34ad7bc6d89376d0b1952dfe49e5efb2a8744d`. The remaining product defect was a hidden label published through a supported SQL string (`''`). The same repair treats unquoted `DISTINCT` as reserved, including the degenerate form, and omits a relationship or entity whose name or entity path repeats a hidden name. The deciding Linux run on the committed archive is `linux/commit/`: `oo-builder` `role_pack` 11 passed, clippy `-D warnings` exit 0. This record does not approve SP07-W06. Independent review is next.

A visibility snapshot is an input. It is not authentication and it is not a grant. The compiler does not execute SQL and it is not a SQL gateway. Parse success is not execution.

The source commit was written after `linux/focused-2` exited 0 and before this evidence package and cleanup. It was not amended. `linux/commit` is the later smoke of that commit.

## Fixed source

| Item | Value |
| --- | --- |
| Commit | `eff1959bf028a65b20b7fca855d79c23e4b188fd` |
| Tree | `2f08d4ecef7ad2f3b617e8a04afcb45f0cf0186f` |
| Parent | `9e3f757fb34746c64c6fca3d60cf0c5e342a1885` |
| Subject | builder: close role-pack SQL string and name closure |
| Reviewed source | `ef34ad7bc6d89376d0b1952dfe49e5efb2a8744d` |
| Review | `95289c50` |
| Parent subject | evidence: record the a173 column-ACL review |

Owned paths in the source commit:

- `crates/oo-builder/src/role_pack.rs`
- `crates/oo-builder/tests/role_pack.rs`
- `contracts/builder/role-pack-v1.md`

The source commit changes those three files only: 405 insertions, 14 deletions. `Cargo.lock` is the same blob as the parent (`8364c36f2c70919cd3a1df18a2e57698a44051a8`). This task did not edit the lock, dependencies, core, another builder, a spec, or the ledger.

Content SHA-256 and git blob, matching `linux/commit/expect-sha256.txt` and `source/blobs.sha256`:

- `crates/oo-builder/src/role_pack.rs` `e9349ccd9b0e4576e0b4dd712e28aba0f5e7562dba72413d431aac3d14a7d544` blob `1327bad464fe8f510cc7baf8566ad71e3d00df2f`
- `crates/oo-builder/tests/role_pack.rs` `278a9a61226fe59df87b971f405d62d0913d63e026ec59c5e36ee9b33c347057` blob `f13d3424381c268a4d2859b76092f7f69221f99c`
- `contracts/builder/role-pack-v1.md` `dce53af7210c4d8c87bb3bff78ac349e56f4be7d432dfe2242a9c9166d47132a` blob `679e8a46a298042f7c1b21df92c81fe790b63b43`
- `crates/oo-builder/src/lib.rs` unchanged `8e138d4f3aaadd32f5b09ba05d97bf44c312e68f647cc64d6b912a855bc36cd1` blob `e5d7d7a4b1b13c8ea1131841c45983ba40fb787d`
- `Cargo.lock` content `45a784dc73ee1ba7531430bb4f32f368f22220271c28fccb84540593953bcffd` blob `8364c36f2c70919cd3a1df18a2e57698a44051a8`

`lib.rs` contains `#![forbid(unsafe_code)]`. `linux/commit/unsafe-lines.txt` records that line. `role_pack.rs` has no `unsafe` token. The test hash uses the existing `sha2` 0.10.9 dev-dependency. `unicode-normalization` 0.1.25 is the crate already in the lock. No crate was added.

`linux/preedit`, `linux/focused`, and `linux/focused-2` hashed the `ef34ad7b` lock `34b7364c3c68b48a7d1c1cab40b659408ce9087f14a15626c38ed2016a63af8e`. The parent moved to the a173 gateway lock before the source commit. `sha2` 0.10.9 and `unicode-normalization` 0.1.25 are in both locks. `linux/commit` is the run on the committed lock.

## What the compiler now does

`scan_texts` decodes a supported single-quoted SQL string before the hidden-label comparison. A `'` opens a string when it is outside a double-quoted identifier and the previous character is not an identifier character. `''` is one apostrophe. The comparison text is NFC after that decode. An unclosed string or an unclosed quoted identifier fails closed and the object is omitted. The copied pack text is not rewritten, so a kept `'it''s visible'` stays spelled that way. Prose `it's` is not a string.

Unquoted `DISTINCT` is in the reserved keyword list. `parse_name` rejects a keyword, so `SELECT DISTINCT mass_kg FROM depot.parcel` and `SELECT DISTINCT FROM depot.parcel` do not parse and the metric is omitted. That degenerate form is not kept as a column named `distinct`.

`select` includes a relationship only when `visible_prose` accepts the relationship name. `entity_fully_visible` requires the same check on the entity name and on `entities/<name>.md`. A failing relationship or entity is omitted. An example whose `rel:` names an omitted relationship is omitted with it. Endpoint allowlisting does not keep the object.

`FINDINGS.md` has the line citations and the fixture.

## What ran

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, already local, `linux/arm64`, `--pull never`. `RUSTUP_TOOLCHAIN=1.94.0`. Observed `rustc 1.94.0 (4a4ef493e 2026-03-02)` host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, `cargo 1.94.0 (85eff7c80 2026-01-15)`. The repo `rust-toolchain.toml` says channel `1.98.1`. The container environment overrides it to `1.94.0`.

One builder at a time. Private network `oo-a174-eef831aa-net` (bridge, `internal=false`). `--cap-drop ALL`, `--security-opt no-new-privileges`, `--user 0:0`, memory 8g, cpus 4, pids 4096. `HOME=/tmp`. Cargo home and target were task volumes `oo-a174-eef831aa-cargo` and `oo-a174-eef831aa-target`. `/tmp` was a container tmpfs `rw,nosuid,nodev,noexec,size=1g`. No host network, host socket, host home, shared cache, or PostgreSQL. Slot B was not used. Host `cargo` and `rustc` were not started.

The pre-edit smoke compiled `git archive` of `ef34ad7b`. `linux/focused` and `linux/focused-2` compiled that archive with the three working-tree files copied on top, still on lock `34b7364c`. `linux/commit` compiled `git archive` of `eff1959b`, lock `45a784dc`.

| Run | Container id | docker exit | What it shows |
| --- | --- | --- | --- |
| `linux/preedit/` | `806a8709aeff789154ffb56745405b8fb73bd39b948489e5ff38c82b284cf7d4` | 0 | Before product edits. Archive `ef34ad7b`. Hashes matched the a172 fixed source. `role_pack` 10 passed. Clippy `-D warnings` exit 0. Offline fetch on the empty cargo volume exited 101, then online fetch exited 0. Preserved. |
| `linux/focused/` | `6386f2f2fe1bcc7bf7a9bb144f5ad711acbf700ef53b4f212af15d7379aa5eef` | 101 | First post-edit compile. Ten existing tests passed. The new test panicked because `assert_absent("source-declared")` matched the relationships intro sentence. Clippy `-D warnings` exit 0. Preserved, including `snapshot/`. |
| `linux/focused-2/` | `8dc5684e359a9f464fba6e968ed63dcea0ee328a1ed878572b2822cd2b4a6674` | 0 | Same three files after the absent token was `declared_fk`. `role_pack` 11 passed. Clippy exit 0. Offline fetch exit 0. Lock `34b7364c`. |
| `linux/commit/` | `aee84d9d3ce40ef13f9ea2f9cb9e01540f7aa10f07a981a40cb21d5294a57920` | 0 | Deciding run for commit `eff1959b`. Hashes matched the committed tree, including lock `45a784dc`. `role_pack` 11 passed. Clippy `-D warnings` exit 0. Offline fetch exited 101 (`postgres-types` was not in the cargo volume), then online fetch exited 0. |

`linux/commit` phase exits: `rustup-clippy` 0, `fetch-offline` 101, `fetch` 0, `test` 0, `clippy` 0, `hash-compare` 0, `worst` 0. `OOMKilled=false`, `Privileged=false`, `CapAdd=null`, `CapDrop=["ALL"]`, `PortBindings={}`, `NetworkMode=oo-a174-eef831aa-net`, memory `8589934592`. Capacity at launch: `own=0 running_rust=0 host_cargo=0 global=0`. Test compile finished in 1.97s. The 11 tests finished in 0.05s. Clippy finished in 1.82s.

Each launch script removed its own container. `container-removed.txt` in each directory records the name.

## Oracle

`ORACLE.md` describes the check. `escaped_sql_label_distinct_and_name_closure` recomputes the pack digest, the manifest SHA-256, and the cache-key SHA-256 with `sha2` and a canonical JSON writer in the test file. The a168 Python oracle under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a168/SP07/role-pack-review/` is unchanged historical evidence. This task did not rerun it and did not add a Python file.

## Limits

Domains, a tokenizer, a read API, search, mount, authentication, a policy adapter, a database re-proof of row security, and publication gates G1–G13 stay open. The manifest still says authentication, authorization, and publication are `not_claimed`, samples are `omitted`, mount is `not_performed`, and `row_security_reproven` is false. No ledger line was edited. Nothing was pushed. Independent review of `eff1959b` was not started.
