# Role-pack bundle review

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a238`. Independent review of frozen commit `0c6e6f1e0efe419e4ed1d34444dd5b3a332074d0` ("builder: publish the role-pack bundle with a non-replacing hard link"). Parent `ac76d5e63a58de2253b4c728dffefaf2e0565601`. Tree `23cb0eaccf51496ec73844421050acc838f17fd0`.

The commit changes only:

- `crates/oo-builder/src/bin/role-pack-bundle.rs` blob `f90ace2010efdd222db85d681b294428e2156100`, content SHA-256 `ee085c9d1a28288efe700a58e772377a909613f04731aed4af3ce25591cc0bf2`
- `crates/oo-builder/tests/role_pack_bundle.rs` blob `a6558dd1cbf1b52dd652a394a184a154c7b0c91e`, content SHA-256 `1143c117e86c1373aa825b2571cbf6473b57b65928c4a1c0a830a28cee7e3f39`
- `contracts/builder/role-pack-bundle-v1.md` blob `8aba171993c3c1f6fe3c139d7f02c19907579558`, content SHA-256 `164388a8e496fd989edcc86f5323257d1863a4d9c7f8058304f828a4b91a2863`

Author evidence is a214 `d20944017bda07ae8a7e95c8acfcfaebb829aa4a` and the repair a221 on this same commit. a214 recorded that a destination appearing between the existence check and `rename` can be replaced. a221 replaced that publication with `std::fs::hard_link`. This review did not edit product sources, manifests, the lock, or the ledger. a207 remains the connector-manifest and root-lock writer.

No `AGENTS.md` is present. The contract is `contracts/builder/role-pack-bundle-v1.md`, `oo.builder.role-pack-bundle/v1`. O-D02 and SP07-W06 stay open. This review does not accept a parent item and does not approve a release.

## What was reviewed

The frozen command reads one `oo.builder.role-pack-command/v1` file, calls `compile_role_pack_command`, and writes one new local bundle. `pack` and `manifest` are the compiler's artifact and manifest bytes. `pack_sha256` and `manifest_sha256` are hashes of those spliced bytes. `pack_digest` is copied. Snapshot identities are copied from the manifest. The fixed status table stays `not_claimed` for authentication, authorization, and publication, and `not_performed` for mount.

Publication is `std::fs::hard_link` from the command's own synced staging file to the output name in the same directory. An existing file, symlink, or directory stays. A pre-existing staging path stays. Two processes that publish different outputs keep their own bytes. Two processes that share one output path produce one file equal to one of the two bundles.

## Linux Rust 1.94

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, rustc `1.94.0` (`4a4ef493e 2026-03-02`), cargo `1.94.0` (`85eff7c80 2026-01-15`). `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. One private bridge `oo-a238-eef831aa-net`. Cargo volume `oo-a238-eef831aa-cargo`. Target volume `oo-a238-eef831aa-target`. No PostgreSQL.

Each builder drops all capabilities, sets no new privileges, memory 6 GiB (`6442450944`), 1 CPU (`NanoCpus=1000000000`), `CARGO_BUILD_JOBS=1`, no host network, and no published ports. `/tmp` is tmpfs `rw,nosuid,nodev,noexec,size=1g`. The source mount is read-only. Recipe origin is `recipes/ORIGIN.md`.

| Run | Result |
|---|---|
| `linux/preoverlay` | Exit 0 before the review oracle. Container `943df3d97a788f69d8df4d40a0cb6f536794532c5e9c17b55fc460a0cb9efd9c`. Archive of `0c6e6f1e`. `overlay-absent.exit` 0. `hash-compare.exit` 0. `fetch-offline.exit` 101, then `fetch.exit` 0. Binary tests 11 passed. Process tests 10 passed in 2.17s. Clippy `-D warnings` exit 0, `clippy.stdout` empty. `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`. Capacity `own=0 global=0 host_cargo=0 cap_global=10`. |
| `linux/proof` | Exit 0. Container `ac366f9a5287cd085cae01c5c3467ea7e0c11ee7f0a9c3cb570beba53d95756b`. Same archive pin. `cargo build` of `oo-builder` and `role-pack-bundle` exit 0. `rustc --edition 2021 -D warnings` of the review oracle exit 0. Oracle exit 0, `review_result=pass`. Clippy of the frozen bin and `role_pack_bundle` exit 0 in 0.07s, `clippy.stdout` empty. Same 6 GiB, 1 CPU, `OOMKilled=false`, `Privileged=false`. |

The proof oracle is compiled from `oracle/review_oracle.rs`. It is not copied into the archive.

## Oracle

`linux/proof/oracle.txt` is the independent run. Inputs are `oracle/kiln-clerk.json` and `oracle/kiln-audit.json`. The compiler binary is the frozen `oo-builder role-pack`. The bundle binary is the frozen `role-pack-bundle`. Digests are this oracle's SHA-256, checked against the empty digest and the `abc` digest before use.

```text
hardlink_tmpfs=supported ino_eq=true nlink=2
hardlink_bind=supported ino_eq=true nlink=2
product_on_tmpfs=published
clerk_file_sha256=2b0e4250b7ff7a4230706122f09c8e93744b68dde61b4b788dac10ef9f22c784
clerk_pack_sha256=692972da9a955f6c6e3027129706c86feaa208dc789ab47ea8ecc7fb1ced1f69
clerk_manifest_sha256=954ff828edca50f8d1156a7d8c9e177ce12ab7b2f8cdef3e8c4e72cd24e20b8e
clerk_pack_digest=50af382d5c4d692db5030ea9f976d4ae564879b009d18d57d5591a6f55908653
audit_file_sha256=82017b513c083fa0e9efec5781c2530a15c5eed2fd92bbb13a0c44205851f25d
audit_pack_sha256=92cc2aa478db0e8c61d3e499e83fb99d1c15fa768092943ffb166d2057e77520
audit_manifest_sha256=0cef43cc1801cb265380e08de1ced97dc676135b75a24a7745205772e05ce828
audit_pack_digest=7a2e476142a854780b22a0df7c294492014f077db76d66abdc08cc2cc99bfef8
pack_sha256_differ=true
sentinel=unchanged
symlink_destination=unchanged
directory_destination=unchanged
preexisting_staging=unchanged
concurrent_same=one_winner_foreign_unchanged
concurrent_distinct=own_outputs_foreign_unchanged
help=no_auth_mount_publish_no_file
product_unsupported=not_mounted
link_window=not_observed tries=4
review_result=pass
```

Clerk snapshot role is `oo_r_kiln_clerk`. Audit snapshot role is `oo_r_kiln_audit`. Both snapshots carry project `kiln`, policy `pol-kiln` version `9`, and knowledge `know-kiln` version `4`. The compiler stdout contains `cache_key` and both principal ids. Neither bundle does.

## Limits

- tmpfs on `/tmp` and the evidence bind (`fakeowner`) both support hard links. A filesystem that refuses `link(2)` was not mounted. `product_unsupported=not_mounted`. The unsupported sentence remains the mapping in `hard_link_unsupported`, re-run by `link_errors_use_static_sentences` in the smoke.
- The external watcher did not see both names during four publishes. `hard_link_shares_the_inode_and_then_drops_the_staging_name` in the smoke is the inode check of the product function. The process check that an existing name stays is the sentinel, the symlink, and the directory.
- The same-destination loser printed either `io.destination` or `io.collision`. The oracle required one of those two exact lines and did not record which one.
- The parent directory is a caller-supplied trusted path. It is not fsynced. A crash can leave the staging name or both names.
- Authentication, authorization, publication, mount, and a tokenizer stay outside this command. A written bundle is not a grant.
- O-D02 and SP07-W06 stay open. This review does not accept a parent item and does not approve a release.
