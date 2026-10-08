# Source inventory

Reviewed by archive. Counts are `git ls-tree -r` at the pin. The host worktree was not the compiler input.

| Name | Commit | Path | Tree | Files |
| --- | --- | --- | --- | --- |
| product | `fbd0467617a3c1514e7ffbef52f1138e20416ac6` | `tools/release-control-api` | `7375587fb4758d41ef02727451daf3473c29cac4` | 27 |
| identity | `a6f92da8a0bf872f8a30efb116c2c7b14463505b` | `tools/control-identity` | `45f330af6e326d8f1cc1038ef47de21031ef751a` | 28 |
| producer | `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3` | `tools/release-coordinator` | `5a3692de6f1e5609d684164a34af672b11c163b6` | 16 |
| oo-builder | `fbd0467617a3c1514e7ffbef52f1138e20416ac6` | `crates/oo-builder` | `40f74d7e9aac9c13e95d1da3d61d689184773c77` | 41 |

Blobs cited in the findings:

- `tools/release-control-api/CONTRACT.md` `dc64a53f5f54b8f4b080ecaa8561a16605d65d34`
- `tools/release-control-api/src/identity.rs` `57d4eeb050886877977dfe677d8022e23993ca41`

Root `Cargo.lock` copied into the builder snapshot: `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474` (`linux/archive/root-lock.sha256`).

Recipe reused without edit: `deploy/trusted-demo/pg/pg-wrap.sh` at `b91b67bee0f603725b2cb7cc9c284588a2699392`, content sha256 `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a` (`pg/pg-wrap.sha256`).

Evidence files and their sha256 values are `SHA256SUMS` in this directory. The sums file lists every other file under this review directory.

Not compiled, recorded only: host `tools/control-identity` status in `linux/build-1/identity-worktree.txt`, and product commit `e2bd526e96175efb52fa1181843fbe991ae7992b`.
