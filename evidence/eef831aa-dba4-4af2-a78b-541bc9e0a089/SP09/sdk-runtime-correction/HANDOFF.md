# a089 handoff

Fixed local commit for independent review. No push, no pull request, no release.

Delivery commit: `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df`
Delivery tree: `db0717173187604182b02d4fc8227b107bddb6d1`
Delivery parent: `6831d6e862303a0ce1c883925068078cbcb78e23`

The evidence update commit's parent is that delivery commit. It changes only this file and `REPORT.md`. A commit cannot store its own hash.

Test archive, immutable and used for every Linux run: `77d957750e18c7e811e845a7717d54372b41839e`, tree `4e3ae4699dc0065f605b48e12e88ee7ce859d09d`. `2980e85af22c3ade05272c66c7d2835b52730322` and `99e00edffd232cbeae95f374568c9ea537cfb478` stay ancestors of that archive. The delivery parent may be a later commit that does not touch `oo-sdk` or the lock. The Linux overlay is that archive plus the owned SDK files only.

This runtime slice is not whole-SDK approval. Supply findings stay open and are not waived:

- `SUP-CDLA-TARGET-ALL`: deny exit 4, `webpki-root-certs` 1.0.9, `CDLA-Permissive-2.0`. Absence from the Linux selected graph is not a target-all license pass.
- `SUP-REFI-OR-POLICY`: `r-efi` is MIT OR Apache-2.0 OR LGPL-2.1-or-later. `AUTHORS` is not a MIT license body. a078 already corrected that record. License-file policy and LGPL-or-later restrictions stay independent.
- `SUP-DUNCE-OR-POLICY` and the native aws-lc conjunctive obligations stay with the dependency and policy work.

`cargo audit` 0 is not supply admission. `ci/deny.toml` and the ISC allow entry are unchanged. No root `Cargo.toml` or `Cargo.lock` change. No new dependency, including no `zeroize` or `secrecy`. Dropping a token still does not wipe process memory.

Probe 2 sha `f780a4f1871266c1824290ed6eaafd785317c5ec0e6d9198d15b29dafb84fb6e` is unchanged. a073 and a084 evidence was not edited. `127.255.255.255` connect behavior is the fixture observation `OBS-LOOPBACK-127-255`. Loopback policy was not changed to make that row green.

Still pending: SP09-W01-a, full AT-API, real Gateway and PostgreSQL, exports, OpenAPI, discover, CLI, amd64, supply admission, and release.

Owned cleanup completed before this evidence commit. Removed containers `oo-a089-msrv-223ee9e2-70e3-4284-9b9f-6d6b961c07b9`, `oo-a089-msrv-red-223ee9e2`, `oo-a089-msrv-green-223ee9e2`, and `oo-a089-msrv-final-223ee9e2`; network `oo-a089-223ee9e2-70e3-4284-9b9f-6d6b961c07b9-net`; volumes `oo-a089-223ee9e2-70e3-4284-9b9f-6d6b961c07b9-cargo` and `oo-a089-223ee9e2-70e3-4284-9b9f-6d6b961c07b9-target`; and `/tmp/oo-a089-223ee9e2-70e3-4284-9b9f-6d6b961c07b9`. No `oo.owner=a089` container remained. Foreign a086, a088, and a064 containers were left in place.
