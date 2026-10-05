# Handoff

Independent review is next. This session did not review its own commit as that review, and it did not mark `specs/open-ontology-development-ledger.md`.

## Commit

Product `0c6e6f1e0efe419e4ed1d34444dd5b3a332074d0` on parent `ac76d5e63a58de2253b4c728dffefaf2e0565601`.

The commit is only:

- `crates/oo-builder/src/bin/role-pack-bundle.rs`
- `crates/oo-builder/tests/role_pack_bundle.rs`
- `contracts/builder/role-pack-bundle-v1.md`

Fixed archive for both Linux runs: `c2db9c430b822a824a1062b6c3c7345924df7dd8`. Repair target `d20944017bda07ae8a7e95c8acfcfaebb829aa4a`.

The green proof hashed the worktree bytes that became those blobs. `source/blobs/` matches `linux/proof/expect-sha256.txt` for the three owned paths. Content SHA-256:

- `ee085c9d1a28288efe700a58e772377a909613f04731aed4af3ce25591cc0bf2` `role-pack-bundle.rs`
- `1143c117e86c1373aa825b2571cbf6473b57b65928c4a1c0a830a28cee7e3f39` `role_pack_bundle.rs`
- `164388a8e496fd989edcc86f5323257d1863a4d9c7f8058304f828a4b91a2863` `role-pack-bundle-v1.md`

The evidence commit adds this directory. Its hash is the commit object. These files do not contain it.

## Reproduce the green check

One Linux arm64 Rust 1.94 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, private cargo and target directories, `--pull never`, `--memory 6g`, `--cpus 1`. Archive `c2db9c430b822a824a1062b6c3c7345924df7dd8` and copy in the three owned files. Commands:

```text
cargo test --locked --offline -p oo-builder --bin role-pack-bundle --test role_pack_bundle
cargo clippy --locked --offline -p oo-builder --bin role-pack-bundle --test role_pack_bundle -- -D warnings
```

The green transcript is `linux/proof/console.log`. The pre-edit smoke, including the red gap oracle, is `linux/preoverlay`. There is no failed proof attempt to replay.

## Review focus

- The bundle splice is unchanged. Publication is `std::fs::hard_link` from the synced staging file to the output path in the same directory.
- `AlreadyExists` is `io.destination`. The existing file, directory, or symlink stays. There is no fallback to `rename`.
- `ErrorKind::Unsupported` or raw code 1 is `io.link` / `the filesystem does not support a hard link`. Other link errors use `the staging file could not be linked`. No real unsupported filesystem was mounted.
- Cleanup is `remove_file` on the staging file this attempt created. A pre-existing staging path is `io.collision` and is left in place. The destination is not removed.
- The pre-edit oracle shows `rename` replacing `SENTINEL-a221-gap`. The inode test shows two names of one inode before the staging name is removed.
- The two-process test accepts either `io.collision` or `io.destination`. The log does not record which one fired. Both processes share one staging name.
- The parent directory is a trusted path. It is not a jail and it is not fsynced. An existing output name stays in place.
- Stderr is one static line. The request body and the path are not copied. Help still refuses authentication, grant, mount, tokenize, cache trust, and publication.

## Still open

Authentication, the policy adapter, filesystem mount, the read API, a real row-security proof against PostgreSQL, tokenizer and file splitting, domain chapters, search, cache trust, and publication gates G1–G13. Whole SP07-W06 and O-D02 remain open. No component was added.

Owned Docker containers from this session were removed by their launch scripts before these reports. Volumes, the private network, and the archive directory are recorded in `CLEANUP.txt`.
