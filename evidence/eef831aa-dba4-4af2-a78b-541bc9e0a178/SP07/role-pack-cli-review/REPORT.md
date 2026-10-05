# Report — a178 role-pack command review

Verdict: **APPROVE LIMITED** the `oo-builder role-pack` command at `af3308c32fa9e90bb29fab970b691d57dbc35279`, tree `ef9b913640afd14cb87c9e79baaa321ebb205560`, parent `44153c488720cd290e9363763c60b0830717a9d6`, subject `builder: add the role-pack command`.

The command reads one `oo.builder.role-pack-command/v1` document from stdin, calls the accepted `compile_role_pack`, and writes that pack. Line citations are in `FINDINGS.md`. No product change is required for this command slice.

## Scope

Reviewed the immutable git archive of `af3308c3`. The moving worktree was not the source. `crates/oo-builder/src/role_pack.rs` content hash is `e9349ccd9b0e4576e0b4dd712e28aba0f5e7562dba72413d431aac3d14a7d544`, the same blob `1327bad464fe8f510cc7baf8566ad71e3d00df2f` a176 approved at `eff1959bf028a65b20b7fca855d79c23e4b188fd`. `Cargo.lock` blob `8364c36f2c70919cd3a1df18a2e57698a44051a8` is the same lock. The product diff of `af3308c3` against `eff1959b` under `Cargo.toml`, `Cargo.lock`, `crates`, `contracts/builder`, and `rust-toolchain.toml` is the five command paths only.

This review did not repeat the a176 closure campaign (`''` decode, unquoted `DISTINCT`, relationship and entity names, F1, F2, F3). Those stay closed on the unchanged compiler. The fresh fixture checks that the command forwards that compiler: one legal metric stays, one hidden column and one hidden dataset stay out, PII notes stay without samples, and the row-security sentences stay unproven.

No product file was edited. No PostgreSQL was started. Slot B's gateway tree and lock were not the archive and were not edited. The oracle is Rust `sha2` 0.10.9 in `overlay/role_pack_a178_review.rs`. It spawns the `oo-builder` binary. It does not call the library hash helper. No Python oracle was written.

Author evidence `1c71e5a7` records the command. That record is the author's tests. This directory is the independent run. a176 review `b30020b7` is the accepted library this command calls.

## Runs

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Toolchain forced with `RUSTUP_TOOLCHAIN=1.94.0`: `rustc 1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`, cargo `1.94.0`. The archive's `rust-toolchain.toml` says channel `1.98.1`. Workspace `rust-version` is `1.94.0`. Each run: `--cap-drop ALL`, `no-new-privileges`, memory 8g, cpus 4, pids 4096, private network `oo-a178-eef831aa-net`, private volumes `oo-a178-eef831aa-cargo` and `oo-a178-eef831aa-target`, `HOME=/tmp`, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=1g`. `--pull never`. Capacity at each launch was `own=0 running_rust=0 host_cargo=0 global=0`.

| Run | Container | docker exit | What it shows |
|---|---|---|---|
| `linux/smoke/` | `5b0851664b352f1c4d680a06ca51deca6dddbb8a6b4950f559a49afb35b81e80` | 0 | Pre-overlay archive. Hash check 14/14. Overlay absent. Author CLI and library tests 0. Clippy `-D warnings` 0. Offline fetch 101 on the empty cargo volume, then online fetch 0. |
| `linux/probe/` | `02d092dcfce16aabcc5b790c3ab2cc92238f416b94a7e58fd3c5d48ddd85fb61` | 101 | Harness. Author tests 0. Oracle 0. Clippy rejected `assert!(JSON_MAX_DEPTH < 40)` in the overlay. Product digests match probe-2. Preserved. |
| `linux/probe-2/` | `609ede57dc26eeb253ae29fdd8511cfc44a797dc76cfa7fa02fb0159c590ba3f` | 0 | Deciding. Product hashes 0. Author tests 0. Oracle 0 (4 passed, 0.04s). Clippy 0. Offline fetch 0 on the warm private volume. |

Smoke author tests, all exit 0: `captured_pg_cli` 3, `review_pg_cli` 4, `role_pack` 11, `role_pack_cli` 4, `structure_draft_cli` 4. Probe-2 ran that same set again, then the independent oracle.

Probe-2 host config: `Privileged=false`, `CapDrop=["ALL"]`, `CapAdd=null`, `NetworkMode=oo-a178-eef831aa-net`, `Memory=8589934592`, `OOMKilled=false`, empty port bindings, tmpfs size 1g. The launch script removed the container. Overlay content hash on that run is `f9cd366883b0e5512450f9f4bcc97369b2dcf84efea77ffca19e17b28a722288`. The preserved probe overlay hash is `7f374d734c0f79a39eea3ba351ccee5a7a239e318cb9e2b8be60cc3862f17b02`.

## Result

The deciding binary's stdout for two roles and a second principal is byte-identical to `compile_role_pack` on the same documents. Clerk and `ada178b` share artifact and pack digest `0adc296da6c6c882d4886792862b7eece542870e53d3673f3804f64a0a153e76`. Their cache keys differ by `principal=ada178` and `principal=ada178b`. Audit digest `409726ad03a0768bcd1097c115ffc69bb996c921ee610c2d05e490ef06301fe2`. The artifact has no principal. Independent `sha2` matched file hashes, byte lengths, `pack_digest`, `cache_key_sha256`, and `manifest_sha256` `48553b60415c64f16d130b41bd5fb3eab5d3c766eb10d9d97c9c9df05cdec1ec`.

Clerk `tables/yard.bale.md` keeps `包号`, `净重`, `货包`, the approved notes, the PII sentence, and the unproven row-security sentence. `metrics.md` keeps `SELECT mass_kg AS total FROM yard.bale`. Glossary keeps `净重口径`. Calendar keeps `本季`. Hidden `vault_bin`, `舱记`, `yard.scale`, `pier.slip`, the owner marker, the sample marker, and the denied row count `717171` are absent. Audit keeps `yard.scale`, `秤台`, and `estimated rows: 41`, and does not keep the bale pack.

Manifest `authentication`, `authorization`, and `publication` are `not_claimed`. `mount` is `not_performed`. `samples` is `omitted`. `row_security_reproven` is false. The temp directory stayed empty.

Stale policy, a foreign compiler id, a foreign effective protocol, a sample field, and a nested unknown field exit 1 with the compiler code and a static sentence. Command protocol `v0`, duplicate keys, unknown fields, missing fields, non-objects, trailing data, depth, node count, oversize input, and non-UTF-8 stdin exit 1 without the marker. No arguments and an unknown command exit 2. An extra argument exits 2 and is not copied. `structure-draft`, `captured-pg-draft`, and `review-pg-draft` still exit 2 on an argument and exit 1 on a bad document under their own diagnostics. Their author suites passed on the same archive.

The visibility object was treated as caller-supplied public data. A successful command did not create a file, mount a pack, authenticate, grant, or publish.

## Still open

Domains, the tokenizer, the read API, search, filesystem mount, authentication, the policy adapter, a database re-proof of row security, and publication gates G1–G13 stay open. This review approves the command slice. It does not approve SP07-W06. No ledger checkbox was marked. No release was cut.
