# Handoff

Owned paths: `tools/selfbuild-control-api/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a373/SP07/selfbuild-control-api/**`. No root manifest change. No push.

G1 and G2 keep the operation IDs in `INTERFACE.md`. The service commit that contains this handoff is the implementation commit. Identity authority for a live call is a typed `getIdentityCurrent` envelope. Mutations also require `Origin` on the deployment allowlist and the CSRF token from the caller's latest current, verified by the identity server before this service calls current again.

## Dependency pins

| Product | Commit | Use |
|---|---|---|
| control-identity contract | `089b00d7f70da1bc42f7e201dfa070748c651d5b` | schemas only, unchanged |
| control-identity product read | `a6f92da8a0bf872f8a30efb116c2c7b14463505b` | private archive, unaccepted |
| review-ledger | `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b` | binary reused, authenticated false |
| builder pack | `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba` | not built this run |
| rules | `e5427c1a18e2ddbb480791d214c76888eb7ca998` | not built this run |
| suggestions | `d22187ea0fce8e420323660841f2da3b447d5de8` | not built this run |

## Still open

Customer IdP, independent acceptance of `a6f92da8`, full role and permission matrix, publication, harness, and parent goal. S0–S10 on the real 100k fixture, including model success, cache, cost, and failure with no review approval and no publication. Producer binaries are absent. The pinned model origin is host loopback, which the bridge builder does not share.

`oo-a373-pg` remains running for the existing databases. The exited `oo-a373-rust` container is removed. Volumes `oo-a373-cargo`, `oo-a373-target`, and `oo-a373-pgdata` stay. `/tmp/oo-a373` holds the private archives and is not a repo path.

Original E56 is not accepted.
