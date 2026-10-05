# Findings

Verdict: **APPROVE_LIMITED**.

Reviewed contract `contracts/builder/role-pack-bundle-v1.md` at content SHA-256 `164388a8e496fd989edcc86f5323257d1863a4d9c7f8058304f828a4b91a2863`. The binary is content SHA-256 `ee085c9d1a28288efe700a58e772377a909613f04731aed4af3ce25591cc0bf2`. Grok 4.7 in this session was the only coder, tester, and reviewer. No subagent and no worktree. Measurements are `linux/proof/oracle.txt` and the smoke transcript `linux/preoverlay/test.stdout`.

## Repaired publication

a214 `FINDINGS.md` says the destination check and `rename` are not a lock, and a name that appears in that gap can be replaced. The frozen binary contains `std::fs::hard_link(` and the smoke script's `no-rename.exit` is 0 because `rename(` is absent from `role-pack-bundle.rs`. `finish_owned` calls `readback_and_link`. `link_no_replace` calls `std::fs::hard_link`. `AlreadyExists` becomes `io.destination` / `the output path already exists`. `ErrorKind::Unsupported` or raw code 1 becomes `io.link` / `the filesystem does not support a hard link`. Other link errors become `io.link` / `the staging file could not be linked`.

The independent sentinel wrote `SENTINEL-a238-keep` before the compiled command. The command exited 1 with the destination line. The sentinel bytes and `KEEP-NEIGHBOR-a238` stayed. The staging name was absent. A symlink destination stayed a symlink and `KEEP-REAL-a238` stayed. A directory destination stayed a directory and `KEEP-DIR-a238` stayed. A pre-existing staging file `STAGING-KEEP-a238` stayed, and the output path was not created. The refusal was `io.collision`.

On `/tmp` tmpfs, an independent `hard_link` of two names shared an inode and `nlink=2`. The compiled command then published a new bundle on that filesystem (`product_on_tmpfs=published`). The evidence bind also shared an inode (`hardlink_bind=supported ino_eq=true nlink=2`). No mounted filesystem returned `EPERM` or `EOPNOTSUPP` from that probe, so the command was not asked to publish on an unsupported filesystem.

`link_window=not_observed tries=4`. A watcher on the output directory did not sample the interval between `hard_link` and `remove_file`. The smoke test `hard_link_shares_the_inode_and_then_drops_the_staging_name` passed on the same image. That test calls the product function, checks one inode and `nlink >= 2`, then checks that removing the staging name leaves the destination inode and the bytes.

## Compiled chain and digests

The oracle ran frozen `oo-builder role-pack` on the kiln clerk and kiln audit requests, then frozen `role-pack-bundle` on those same files. The bundle `pack` bytes equal the compiler `artifact` bytes. The bundle `manifest` bytes equal the compiler `manifest` bytes. The oracle's SHA-256 of those spans equals `pack_sha256` and `manifest_sha256`. `pack_digest` equals the command field and the field inside the spliced pack, and it differs from `pack_sha256`.

Clerk file SHA-256 `2b0e4250b7ff7a4230706122f09c8e93744b68dde61b4b788dac10ef9f22c784`, pack SHA-256 `692972da9a955f6c6e3027129706c86feaa208dc789ab47ea8ecc7fb1ced1f69`, manifest SHA-256 `954ff828edca50f8d1156a7d8c9e177ce12ab7b2f8cdef3e8c4e72cd24e20b8e`, pack digest `50af382d5c4d692db5030ea9f976d4ae564879b009d18d57d5591a6f55908653`, pack length 5543, manifest length 1569.

Audit file SHA-256 `82017b513c083fa0e9efec5781c2530a15c5eed2fd92bbb13a0c44205851f25d`, pack SHA-256 `92cc2aa478db0e8c61d3e499e83fb99d1c15fa768092943ffb166d2057e77520`, manifest SHA-256 `0cef43cc1801cb265380e08de1ced97dc676135b75a24a7745205772e05ce828`, pack digest `7a2e476142a854780b22a0df7c294492014f077db76d66abdc08cc2cc99bfef8`, pack length 5046, manifest length 1571.

The two `pack_sha256` values differ. Clerk snapshot role is `oo_r_kiln_clerk`. Audit snapshot role is `oo_r_kiln_audit`. The other snapshot fields match the manifest identities `kiln`, `ontology-a238`, `catalog-a238`, `pol-kiln` `9`, and `know-kiln` `4`. Statuses are the eleven-field table, including `authentication` `not_claimed`, `authorization` `not_claimed`, `publication` `not_claimed`, `mount` `not_performed`, and `row_security_reproven` false.

The clerk bundle contains `tile-mark-a238`, `kiln.tile`, and `Estimated row count is omitted.` It omits `hearth-mark-a238`, `kiln.hearth`, `yard.pad`, `pad-mark-a238`, `vault_mark`, and `owner-a238-do-not-copy`. The audit bundle contains `kiln.hearth`, `hearth-mark-a238`, and `estimated rows: 41`. It omits `tile-mark-a238` and `kiln.tile`. The compiler result contains `cache_key`, `prin-clerk-a238`, and `prin-audit-a238`. Neither bundle contains those strings or the substring `principal`.

Help exits 0 and contains the refusal to authenticate, grant, mount, tokenize, trust a cache, or publish. It states that publication is a same-directory hard link and that an existing output path is not replaced. It creates no file. A combined `--help` and `--request` exits 2 with the static usage line and does not echo `marker-a238-do-not-echo`.

## Concurrent outputs

Two compiled processes with different output names both exited 0. Each file matched that role's earlier bundle. `KEEP-FOREIGN-a238` stayed. Staging names were absent.

Two compiled processes with one shared output name produced one success. The shared bytes equalled exactly one of the two reference bundles. The loser exited 1 with either the destination line or the collision line. The foreign file stayed. The staging name was absent.

## Bounds

- Same-directory `hard_link` is the same-filesystem strategy. The command has no byte-copy fallback and no `rename` fallback.
- The unsupported-filesystem sentence was exercised by `link_errors_use_static_sentences` on raw codes, including raw code 1 and the raw-95 error that this toolchain classifies as `ErrorKind::Unsupported`. This session did not mount a filesystem that returns that error from `link(2)`.
- The parent path is caller-supplied. The command does not canonicalize, chroot, or rewrite `..`. It does not fsync the parent.
- Two `role-pack-bundle` processes that share one output path share one staging name. One success and one refusal is the observed outcome.
- `clippy --bin role-pack-bundle --test role_pack_bundle -- -D warnings` exited 0. The eleven binary unit tests ran under `cargo test`.
- Authentication, authorization, publication, mount, samples, domains, file splitting, search, a database re-proof of row security, cache trust, and a tokenizer stay outside this command. A written bundle is not a grant.
