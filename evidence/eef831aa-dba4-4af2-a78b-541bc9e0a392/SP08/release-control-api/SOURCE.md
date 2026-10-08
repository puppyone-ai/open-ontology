# SOURCE

Twenty-seven files under `tools/release-control-api/`. No root `Cargo.toml` or root `Cargo.lock` change. The child lock has 297 lines matching `name = `.

SHA-256 of the working tree at the time of this packet:

```
96a343d819cab557228fc802af34a9893a8a8ec1009dd161c84b2ae9c783d747  tools/release-control-api/CONTRACT.md
e8ef1c1838d1768112a4abbd4bc8887d524520dec6426a6cd653424b230e76fd  tools/release-control-api/Cargo.lock
827f8fa91b71356811669d6fbcbb851745c157d589f3a90d66a4c59de4c93ac2  tools/release-control-api/Cargo.toml
54626d16d826fd40ee5a5cb561ed0f3a2b60cd0cceeb314b0d1612442abad1c1  tools/release-control-api/migrations/oo_release_api_v1.sql
de281a35aa6f6a75e2a6c4cb8e1a9fb3d2f76dc20799db164c024d1bd4239946  tools/release-control-api/openapi/release-control-api.openapi.yaml
1be116060e5302d559b313917d6ba2e265dc51969605dc926bd6c178042d6682  tools/release-control-api/schema/active.schema.json
5cef83b07e22b9accd10b115fd2537121ba18022984cf6d9ccef7e5b90238296  tools/release-control-api/schema/create-release.request.schema.json
ca8db88444bdfe9e47c30b6a25ba7d106f73b6841c0e711e5040a78c7d41d892  tools/release-control-api/schema/error.schema.json
a83aac2d6c63f9ecb4978cdb2576b1d9f6d2cf6bebf698b90094bac99bf4802f  tools/release-control-api/schema/release.schema.json
1a35a98848010e9f171f89d497a409f9faf91be87a0db7bd60efabb5d59d618f  tools/release-control-api/schema/role-pack.schema.json
3484132fd3fb8c273771b720307f87215d4b52a8db05e020c83a8bcc2c66d267  tools/release-control-api/schema/rollback.request.schema.json
e3201a6603be3aca1360198db298dc4bc1932dee55e3e7ecd58d44755c8665db  tools/release-control-api/src/config.rs
3cbccae599b834ba6cd576e7f47abfff963d415a43884190a17c0c229a5eb391  tools/release-control-api/src/error.rs
b042c9f082941f651b2421926acd4d972370a7469697df9702fd949f41516b26  tools/release-control-api/src/http.rs
6bc58e12954ba4482b9c7213644d48826654633376f8e30934cbab57d3d4b97c  tools/release-control-api/src/identity.rs
88bdcfd5ebee084f0b623920cec37719cbea251677fa2e3b9dbed45c5f1c0322  tools/release-control-api/src/ids.rs
7343470f76dd5e05e5328f5dec276306a07c686fa88175845a671b0eab42fab3  tools/release-control-api/src/jsonutil.rs
790925f57b2acdcec899e56de48fbae0d7439b3e41527ff859fc5a93b970b373  tools/release-control-api/src/lib.rs
094d1ac2d74f03b01a9936d3aa30f65ab7319ac9fd86118453bad29205ceb883  tools/release-control-api/src/main.rs
8340fd4d7df0c9d4656c75c956aab983eb4dee5cdfe7110068a4f8aedcb4f193  tools/release-control-api/src/pack.rs
02f6bf032700daac8f92df18800fb14d2f3d6878af87ac4691602c9153438a0b  tools/release-control-api/src/sha256.rs
99f95d2da30b40eb2774d4edd5f475422cd32d2d74251b1dbd2e14e308c3dec9  tools/release-control-api/src/store.rs
a1f72ad1c7e5074975c6a9eb9f88d4b3e4a446eb943e8534b8300d0aa359e9d6  tools/release-control-api/src/tls.rs
7692a3f5b42f9a6f1557976de2e21a70e32c031475ce2267c274af0b00ebbbdc  tools/release-control-api/tests/contract.rs
5f4551cadec37e121658d139d58525163c9c8f98dcbe84736bd1cf8209fa8ab0  tools/release-control-api/tests/live/idp.rs
3d0dcc7213cabc06063266609fc8dd7b22e542199e9bd0e55662e60d6501e169  tools/release-control-api/tests/live/main.rs
1234f5815e188ad17b6796966365939ede07092fec02e55d3b9e0550495e0333  tools/release-control-api/tests/live/shop.rs
```

The lines are file contents, not git blob ids. `Cargo.toml` pins edition 2024, rust-version 1.94.0, and exact versions already present in the root lock. `unsafe_code` is forbidden. The path dependency `release-coordinator` is `../release-coordinator` version `=0.1.0`. Dev-dependencies repeat the crates the integration tests link: `control-identity`, `oo-builder`, `ring`, `base64`, and the HTTP and Postgres crates.
