# Handoff

Verdict: **APPROVE_LIMITED**.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a238`. Path `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a238/SP07/role-pack-bundle-review/`. This review did not edit product sources, manifests, the lock, or `specs/open-ontology-development-ledger.md`.

## Frozen source

Commit `0c6e6f1e0efe419e4ed1d34444dd5b3a332074d0`. Parent `ac76d5e63a58de2253b4c728dffefaf2e0565601`. Tree `23cb0eaccf51496ec73844421050acc838f17fd0`.

Owned blobs, also `source/blobs/` and both `expect-sha256.txt` files:

- `ee085c9d1a28288efe700a58e772377a909613f04731aed4af3ce25591cc0bf2` `role-pack-bundle.rs`
- `1143c117e86c1373aa825b2571cbf6473b57b65928c4a1c0a830a28cee7e3f39` `role_pack_bundle.rs`
- `164388a8e496fd989edcc86f5323257d1863a4d9c7f8058304f828a4b91a2863` `role-pack-bundle-v1.md`

The compiler pinned by the proof archive, and not edited here, is `role_pack.rs` SHA-256 `e9349ccd9b0e4576e0b4dd712e28aba0f5e7562dba72413d431aac3d14a7d544` and `oo-builder.rs` SHA-256 `9fb2562bf81892436f81087c719723a48f21395fb7134e3933a69f66d8807b3f`.

## Reproduce

One Linux arm64 Rust 1.94 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, private cargo and target volumes, `--pull never`, `--memory 6g`, `--cpus 1`. Archive `0c6e6f1e0efe419e4ed1d34444dd5b3a332074d0`. Smoke, before the oracle:

```text
cargo test --locked --offline -p oo-builder --bin role-pack-bundle --test role_pack_bundle
cargo clippy --locked --offline -p oo-builder --bin role-pack-bundle --test role_pack_bundle -- -D warnings
```

Proof builds `oo-builder` and `role-pack-bundle`, compiles `oracle/review_oracle.rs` with `rustc --edition 2021 -D warnings`, and runs it with those two binaries. The oracle reads `oracle/kiln-clerk.json` and `oracle/kiln-audit.json`. The transcript is `linux/proof/oracle.txt`. The smoke transcript is `linux/preoverlay/console.log`.

## What the next reader can rely on

- The bundle stores the compiler artifact and manifest bytes. Independent SHA-256 values match `pack_sha256` and `manifest_sha256`. `pack_digest` is copied and differs from `pack_sha256`.
- An existing regular file, symlink, or directory at the output path stays. A pre-existing staging file stays.
- Concurrent distinct outputs keep their own bytes and leave a foreign neighbor unchanged. Concurrent publication to one path yields one winner and one static refusal.
- tmpfs and the evidence bind support hard links. The product published on tmpfs.
- Help refuses authentication, grant, mount, tokenize, cache trust, and publication.

## Still open

A live filesystem that refuses hard links was not mounted. The external watcher did not sample the two-name window. The parent directory is not fsynced. Authentication, the policy adapter, filesystem mount, the read API, a row-security proof against PostgreSQL, tokenizer and file splitting, domain chapters, search, cache trust, and publication gates G1–G13 stay open. O-D02 and SP07-W06 stay open. No component was added.

Owned containers `oo-a238-smoke` and `oo-a238-proof` were removed by their launch scripts. Volumes, the private network, and the archive directory are recorded in `CLEANUP.txt`.
