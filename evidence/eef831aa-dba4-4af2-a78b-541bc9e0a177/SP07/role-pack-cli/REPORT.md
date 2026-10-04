# a177 role-pack command

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a177`. Fixed library base `eff1959bf028a65b20b7fca855d79c23e4b188fd` (tree `2f08d4ecef7ad2f3b617e8a04afcb45f0cf0186f`), the bounded role-pack compiler independently approved limited by a176. Source commit `af3308c32fa9e90bb29fab970b691d57dbc35279`, parent `44153c488720cd290e9363763c60b0830717a9d6`, tree `ef9b913640afd14cb87c9e79baaa321ebb205560`. Subject: `builder: add the role-pack command`.

`Cargo.lock` blob at that commit is `8364c36f2c70919cd3a1df18a2e57698a44051a8`, the same blob as the fixed base. The dirty worktree lock was not staged. `crates/oo-builder/src/role_pack.rs` content hash remains `e9349ccd9b0e4576e0b4dd712e28aba0f5e7562dba72413d431aac3d14a7d544`.

## Command

`oo-builder role-pack` takes no arguments. It reads one `oo.builder.role-pack-command/v1` document from stdin. The core scanner `oo_core::parse_json_value` runs first. Unknown top-level fields are rejected. `effective_oos` and `visibility` must be objects. The command canonicalizes those two objects and calls the accepted `compile_role_pack`. Closure, SQL scanning, and pack hashing stay in that function.

Stdout is one JSON object, sorted keys, no trailing newline. `artifact` bytes are `RolePack.artifact`. `manifest` bytes are `RolePack::file("manifest.json")`. File text is `artifact.files[].text`. `pack_digest`, `manifest_sha256`, `cache_key`, and `cache_key_sha256` are copied from that `RolePack`.

The visibility object is caller-supplied public data. The contract and the usage text say it is not a live permission proof. The command leaves the compiler manifest statuses unchanged: `authentication`, `authorization`, and `publication` are `not_claimed`; `mount` is `not_performed`; `samples` is `omitted`; `row_security_reproven` is false.

Exits are 0, 1, and 2. Stderr is `oo-builder: role-pack: <code> <message>` with a static message. The request body and extra arguments are not copied. The shared stdin reader and stdout writer are reused. A non-UTF-8 stream and a read failure keep the catalog-evidence codes those helpers already use.

The process does not write a pack file, mount a filesystem, authenticate, grant, or publish. Contract: `contracts/builder/role-pack-cli-v1.md`.

## Linux arm64 Rust 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. `rustc 1.94.0` (`4a4ef493e3a1488c6e321570238084b38948f6db`), `cargo 1.94.0`. Private network `oo-a177-eef831aa-net`, private cargo and target volumes, `HOME=/tmp`, `CARGO_HOME=/cargo-home`, `CARGO_TARGET_DIR=/target`. No PostgreSQL. No privileged mode, host network, or host home. The pinned image was not pulled during these runs (`--pull never`).

| Run | Archive | worst | What it shows |
|---|---|---|---|
| `linux/preedit` | `eff1959b` | 0 | Existing CLI tests before the command existed. `role-pack-command.txt` is `absent`. Container `df848d9a5e56880bd3ee787b872d024e3f06202f62c508e4de6bdef526f80ce6`. |
| `linux/focused` | overlay | 101 | Preserved. Certified fixture metric lacked `default_anchor`. Compiler returned `oo.builder.role-pack/v1/oos.invalid` at `/metrics/0`. Clippy component was absent in that container. Container `f55d90d46d025c05336d224d88a28ef3a46c3a705912fecf46b33eca595a36cf`. |
| `linux/focused-2` | overlay | 101 | Preserved. Clippy exit 0. The legal SQL was absent because `mass_kg` was also a column on the ungranted scale dataset, so the accepted scanner omitted the metric. Container `eb5eab783c9f28870615a2f529392fd56e6ef1b85b416c3e8b02871b00fc7e20`. |
| `linux/focused-3` | overlay | 0 | Fixture uses scale column `reading_kg`. CLI, library, and the three existing command tests passed. Clippy `-D warnings` exit 0. Container `6e24ff3321d026749610ac8ff88dc1b1c744d529f808919557f823a3c2426fc1`. |
| `linux/commit` | `git archive af3308c3` | 0 | Deciding run. Same tests and clippy. Container `fc8e513711af4cb71b157db3dc42285efd503eebff126e04dee706406cd93305`. |

Product diff of `af3308c3` against `eff1959b` under `Cargo.toml`, `Cargo.lock`, `crates`, `contracts/builder`, and `rust-toolchain.toml` is the five owned paths only.

Deciding `cargo test --locked --offline -p oo-builder` results, all exit 0:

- `structure_draft_cli`: 4 passed
- `captured_pg_cli`: 3 passed
- `review_pg_cli`: 4 passed
- `role_pack`: 11 passed
- `role_pack_cli`: 4 passed

`cargo clippy -p oo-builder --all-targets -- -D warnings` exit 0. The only `unsafe` line in the new module, the binary, and `lib.rs` is `#![forbid(unsafe_code)]`.

## Two-role fixture on the deciding binary

Dumps: `linux/commit/out/clerk.stdout.json` and `audit.stdout.json`. `outcome.json` records:

- clerk `pack_digest` `1ab9c3bc971f87aab7d71b400432de2aacee3804579b4870fa93e5cd19854522`
- audit `pack_digest` `7add1ff8fa967b47e6cab4ac3a187ed885e68b86902e5d1b05a59201f14242be`

The digests differ. Both stdout objects use protocol `oo.builder.role-pack-command/v1` and end without a newline. Manifest statuses on both are the unchanged `not_claimed` / `not_performed` / `omitted` / `row_security_reproven: false` set. The clerk cache key contains `principal=ada177`. The audit cache key contains `principal=aud177`. Neither artifact contains its principal.

Clerk file texts keep `SELECT mass_kg AS total FROM wharf.crate`, `箱号`, `净重`, `本季到港重量`, `badge note stays`, `Samples are omitted because pii is personal.`, `净重口径`, `本季`, `one row per crate`, `Estimated row count is omitted.`, and `did not re-prove row security`. Clerk stdout omits `vault_bin`, `舱记`, `秤台`, `wharf.scale`, `pier.berth`, `泊位单`, `616161`, `191919`, `hidden_mass`, `owner-a177-do-not-copy`, `berth-hidden-a177`, `crate_scale`, `reading_kg`, `scale_id`, `most_common_vals`, and `pii-sample-do-not-echo-a177`. The legal row-security sentence still contains the token `most_common`. The absence check uses `most_common_vals`.

Audit file texts keep `wharf.scale`, `秤台`, and `estimated rows: 23`. Audit stdout omits `wharf.crate`, `箱号`, `vault_bin`, `舱记`, `badge`, `工牌`, the hidden row counts, `kept_mass`, `hidden_mass`, the owner marker, and `personal`.

The test's independent oracle hashes each file with `sha2` 0.10.9, builds the canonical `{bytes, path, sha256}` array, and checks that digest against `pack_digest` and stdout. It also hashes the manifest bytes and the cache key. It does not call the library hash helper. Details are in `ORACLE.md`.

Stale policy stderr, exit 1, empty stdout: `oo-builder: role-pack: oo.builder.role-pack/v1/binding.policy the snapshot policy version does not match the effective document`. The version marker `stale-a177-secret` is absent. A top-level `samples` field returns `oo.builder.role-pack/v1/unsupported.field` and does not echo `pii-sample-do-not-echo-a177`.

The temp working directory was empty after both successful commands.

## Outside this slice

Domains, the tokenizer, the read API, search, filesystem mount, authentication, the policy adapter, a database re-proof of row security, and publication gates G1–G13 stay open. This command does not finish SP07-W06. A successful command is not SQL execution and not a grant. The development ledger was not edited. Nothing was pushed.
