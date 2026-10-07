# Recipe origin

Archive commit for this repair proof: `93f5b8aea78d0d745334da0bc5352afdb86412e2`.
`pack_catalog.rs` blob at that commit is `9848dcafc4c36b6512240f20fec1b09b1146c02a`, the same blob as `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c`.
The pre-edit `http_gateway_live.rs` blob is `3035942b4c97d487dad2dcd7ba0f6d5f98b8a352`.

Copied from the a341 smoke-3 packet. Byte-identical copies: `pg/pg-wrap.sh` `eda1fb4698a9699bfcc2fe581bcb12bf727715708d1e89ef7f13a22b870e08f8`, `pg/init-roles.sh` `de6973fa11522f486f544716a77aa1158ce390f1a879274dd62b4ad47238621a`, `linux/smoke/extract-libclang.sh` `90a05dd88284e19ec7f34ef2f3280e37a8972e39027ab8cfab248faba2b51666`.

`pg/up.sh` origin `413472f33cbb1c150da826023408b7da1e541d644aa077a54b96fd292e411654`. The only change is the evidence directory `gateway-live-repair/pg`. Database `oo_a341`, SAN `DNS:oo-a341-pg`, and alias `oo-a341-pg` stay.

`linux/smoke-3/launch.sh` origin `28362260279f7e036866bc31f3fd5848371375f7e194bfc3ffa9866bb1a4bef5`. Changes: evidence path, archive commit above, container `oo-a341-live-repair`, overlay mount of `http_gateway_live.rs` only, fixture tree at `/fixture-src`, and both `OO_A326_*` and `OO_A341_*` environment variables. Memory 6g, 1 CPU, `--pull never`, `--cap-drop ALL`.

`linux/smoke-3/inside.sh` origin `d31453f4ef521f206ac3a931bd8d20ea5e215162ffd76e79066a5bfaf9ead0e3`. Changes: the archive must still call `fixture_artifact`; pre-edit smoke is `pack_catalog`, `http_gateway_loopback`, and `http_gateway_live` without `--ignored`; the overlay is only the live test; focused Clippy is the gateway bin, lib, and the three HTTP tests; then `OO_A326_LIVE=1 --ignored` and the existing `http_gateway_pg_live --ignored` entrypoint.

Contract SHA-256 before the proof: `b3b3a6daaaed24c5e74435ede846c409fa0ea876a6ffadca5f340f3e6be6340f`.
Overlay live-test SHA-256 before the proof: `59e0a940f6fed9f63e7ebc0da37ddc189f287bab17e50ca794b5316eb41c9059`.
