# a170 role Pack dependency closure repair

Source commit `ef34ad7bc6d89376d0b1952dfe49e5efb2a8744d`, tree `9f2f6d353a957cc400c7be01d575599260117fe4`, parent `06a99f1181b648863975eb214c14f949d835a67e`.

The repair is the three files named below. Review `d61f9a4c` (blobs `5bbc6bfe`) requested changes on `3dadf678ff0fbfdc1ddd2ac4144ee68cb2ba772c` for three closure leaks. The deciding Linux run on these bytes is `linux/focused-2/`: `oo-builder` `role_pack` 10 passed, clippy `-D warnings` exit 0. This record does not approve SP07-W06. Independent review is next.

A visibility snapshot is an input. It is not authentication and it is not a grant. The compiler does not execute SQL and it is not a SQL gateway.

## Fixed source

| Item | Value |
| --- | --- |
| Commit | `ef34ad7bc6d89376d0b1952dfe49e5efb2a8744d` |
| Tree | `9f2f6d353a957cc400c7be01d575599260117fe4` |
| Parent | `06a99f1181b648863975eb214c14f949d835a67e` |
| Subject | builder: close role-pack visibility for quoted names and labels |
| Reviewed source | `3dadf678ff0fbfdc1ddd2ac4144ee68cb2ba772c` |
| Review | `d61f9a4c` |
| Workspace base before this commit | `06a99f1181b648863975eb214c14f949d835a67e` |

Owned paths in the source commit:

- `crates/oo-builder/src/role_pack.rs`
- `crates/oo-builder/tests/role_pack.rs`
- `contracts/builder/role-pack-v1.md`

Content SHA-256 and git blob, matching `linux/focused-2/snapshot-sha256.txt` and `source/blobs.sha256`:

- `crates/oo-builder/src/role_pack.rs` `3ef960fe13c4dda5f7ca82e9e99f06b57d28cae8b63e6287319e53fe928e4901` blob `f7e4d22be214a0c81ee35952f0a58b1b0519e43b`
- `crates/oo-builder/tests/role_pack.rs` `05a90ea74708818b3462e2d5230392c6db9a9c7c9d88346966d3fd9736bf372f` blob `f6e5521d5f2d4b2e45434175bd59999a38679f52`
- `contracts/builder/role-pack-v1.md` `8e2a578e4434550f179e771e0097a9d406c1e009b64204647f7f4b2e7cf8d62c` blob `516d7ba78c5eed4a65261caeaad69ad3a9ec678f`
- `crates/oo-builder/src/lib.rs` unchanged `8e138d4f3aaadd32f5b09ba05d97bf44c312e68f647cc64d6b912a855bc36cd1` blob `e5d7d7a4b1b13c8ea1131841c45983ba40fb787d`

`lib.rs` contains `#![forbid(unsafe_code)]`. `linux/focused-2/unsafe-lines.txt` records that line. `role_pack.rs` has no `unsafe` token. `Cargo.lock` content SHA-256 `34b7364c3c68b48a7d1c1cab40b659408ce9087f14a15626c38ed2016a63af8e` is the same hash a168 recorded. No dependency was added. The test hash uses the existing `sha2` 0.10.9 dev-dependency.

## What the compiler now does

User text is copied only when `visible_prose` accepts it. That includes `data_type`, `unit`, labels, descriptions, calendar phrase labels, calendar anchor names, and the anchor expressions a phrase renders. A failing column field is left blank and the allowlisted column row stays. A failing calendar anchor or phrase is omitted. A metric, example, or glossary term that fails is omitted.

Hidden names and labels are matched after Unicode NFC (`unicode-normalization` 0.1.25, the crate already in `oo-builder`). ASCII identifiers compare case-insensitively. Non-ASCII text is not case-folded. Quoted identifiers use PostgreSQL doubled quotes (`""` is one quote). An unclosed quote fails closed. A non-ASCII hidden label matches when the NFC text contains that label, and a decoded quoted identifier matches that label by equality.

Verified SQL is `metric.sql`, example `sql`, and `control_total.sql`. Each must parse the subset in `contracts/builder/role-pack-v1.md` or the metric or example is omitted. The subset covers the existing analyst and support statements plus a quoted `UNION ALL`. `JOIN`, comments, semicolons, subqueries, `WITH`, `OR`, `DISTINCT`, casts, and `SELECT *` are omitted. Control SQL is parsed for closure and is not copied. Parse success is not a grant. Prose is not treated as authorized SQL.

`FINDINGS.md` has the line citations and the fixture.

## What ran

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, already local, `linux/arm64`, `--pull never`. `RUSTUP_TOOLCHAIN=1.94.0`. Observed `rustc 1.94.0 (4a4ef493e 2026-03-02)` host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, `cargo 1.94.0 (85eff7c80 2026-01-15)`. Debian 13. The repo `rust-toolchain.toml` says channel `1.98.1`. The container environment overrides it to `1.94.0`.

One builder at a time. Private network `oo-a170-eef831aa-net` (bridge, `internal=false`). `--cap-drop ALL`, `--security-opt no-new-privileges`, `--user 0:0`, memory 8g, cpus 4, pids 4096. `HOME=/tmp`. Cargo home and target were task volumes `oo-a170-eef831aa-cargo` and `oo-a170-eef831aa-target`. `/tmp` was a container tmpfs `rw,nosuid,nodev,noexec,size=1g`. No host network, host socket, host home, shared cache, or PostgreSQL. Slot B was not used. Host `cargo` and `rustc` were not started.

The pre-edit smoke compiled `git archive` of `3dadf678`. Post-edit runs compiled `git archive` of `06a99f11` with the three working-tree files copied on top. `linux/focused-2/snapshot-sha256.txt` matches the committed content hashes above.

| Run | Container id | docker exit | What it shows |
| --- | --- | --- | --- |
| `linux/preedit/` | `63e21b8654a4083fc6a00426861eb9a92547a8b38e66bacf1f0dc0174c966e51` | 0 | Before product edits. Archive hashes matched. `role_pack` 9 passed. Clippy `-D warnings` exit 0. Offline fetch on the empty cargo volume exited 101, then online fetch exited 0. Preserved. |
| `linux/focused/` | `151d72fefbec3267c4e56e3edf8a86355db70be9ca7f9fdb3d8243fcf19ffa41` | 101 | First post-edit compile. The nine existing tests passed. The new closure test panicked because `assert_absent("ex-quoted")` matched the kept id `ex-quoted-visible`. Clippy rejected `chars().any(\|ch\| !ch.is_ascii())` as `needless_character_iteration`. Preserved. |
| `linux/focused-2/` | `1ded18353e936686b6bdaeb9d5ddafc832f03dd07d0e5b18a54c30696ab51f45` | 0 | Deciding run for commit `ef34ad7b`. `role_pack` 10 passed. Clippy `-D warnings` exit 0. Offline fetch exit 0. |

`focused-2` phase exits: `rustup-clippy` 0, `fetch-offline` 0, `fetch` 0, `test` 0, `clippy` 0, `worst` 0. `OOMKilled=false`, `Privileged=false`, `CapAdd=null`, `CapDrop=["ALL"]`, `PortBindings={}`, `NetworkMode=oo-a170-eef831aa-net`, memory `8589934592`. Capacity at launch: `own=0 running_rust=0 host_cargo=0 global=0`.

Each launch script removed its own container. `container-removed.txt` in each directory records the name.

## Oracle

`ORACLE.md` describes the check. `closure_keeps_visible_text_and_omits_hidden_references` recomputes the pack digest, the manifest SHA-256, and the cache-key SHA-256 with `sha2` and a canonical JSON writer in the test file. The a168 Python oracle under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a168/SP07/role-pack-review/` is unchanged historical evidence. This task did not rerun it and did not add a Python file.

## Limits

Domains, a tokenizer, a read API, search, mount, a database re-proof of row security, and publication gates G1–G13 stay open. The manifest still says authentication, authorization, and publication are `not_claimed`, samples are `omitted`, and `row_security_reproven` is false. No ledger line was edited. Nothing was pushed.
