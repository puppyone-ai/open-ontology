# HANDOFF

Owned paths for this slice:

- `crates/oo-gateway/src/http_gateway.rs`
- `crates/oo-gateway/tests/http_gateway_tls.rs` (new)
- `testing/real-db/gateway-tls/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a350/SP06/gateway-tls/**`

`crates/oo-gateway/src/bin/oo-gateway.rs` is unchanged. Blob `3fa14007bd293612771a8aecf9794a405b5a8107`.

Proof-6 measured these working-tree blobs, and the commit must still match them:

- `http_gateway.rs` `969e655d7458d8b679933ffb6f361f08741cabea`
- `http_gateway_tls.rs` `39958a114b0e4f84f258a90b8c06ee9cb4507aeb`

Launch HEAD was `96fc054598a916737452e74bb9ddaa46180057bb`. Lock sha256 `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`.

## Behavior that proof-6 exercised

Loopback remains the development transport when `transport` is absent or `"loopback"`. TLS is a separate listener. Invalid TLS material fails before `listen.addr`. The peer workload string comes from the verified leaf URI. Metadata lookup stays an external-client PostgreSQL lookup of an `ooct_` token. Proxy certificate headers are refused on both transports. The passing live test is `verified_peer_metadata_uses_the_compiled_pack_and_the_leaf_uri`.

Deploy notes are `testing/real-db/gateway-tls/REPRO.md`. The passing invocation is `bash testing/real-db/gateway-tls/linux/proof/launch.sh proof-6`. Do not reuse `linux/proof` through `linux/proof-5`; those consoles are the failures.

## Withheld

Do not mark E48, SP06, or the parent goal accepted from this packet. OAuth, IdP, token issuance, and full session support are out of this slice. A certificate does not become a privileged role. No production credential was used.

## Resources

a350 containers, the cargo volume, the target volume, the network, the PostgreSQL volume, and `/tmp/oo-a350-eef831aa-dba4-4af2-a78b-541bc9e0a350/secrets` were removed after proof-6. Foreign trees were not cleaned. One other rust builder, `oo-a355-sbs-querypg`, was already running and was left alone.

## Check after commit

`git hash-object` of the two source files at HEAD must equal the blobs above. `git status` for the owned paths must be clean. This handoff does not embed the new commit id; read it from `git rev-parse HEAD` after the owned-path commit.
