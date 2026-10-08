# Pack compiler to approved query

UUID eef831aa-dba4-4af2-a78b-541bc9e0a399. Rust 1.94.0, linux/arm64. This is the producer result for consumers 389, 373, 397, and the frontend. It is not an Agent tool-calling run and it is not parent or goal acceptance.

## Contract

A legal policy id is the existing slug or exactly 64 lowercase hex digits. Uppercase, the wrong length, non-hex, and path or control characters still fail. Knowledge stays on the existing slug. The compiler copies `know-a317` and `know_a317` unchanged. Dialogue accepts only its no-hyphen slug, so `know-a317` is still `config` before any socket. The intersection id used in the live pack is `know_a317`.

`endpoint.transport` defaults to production: HTTPS and a bearer of 16 to 256 visible ASCII bytes. The value `development_local` is the only mode that calls `http://127.0.0.1:<port>` and may use a bearer of exactly 6 bytes. Lengths 1–5 and 7–15 stay illegal. `contracts/builder/role-pack-v1.md` and `contracts/builder/reviewed-effective-oos-v1.md` were not edited.

## Live loop

The private database `oo_a399` was loaded once with `generate_series(1, 90000)`. It is not the a389 100k campaign. `role-pack-bundle` compiled the pack. Policy id `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`. Knowledge id `know_a317`. Pack digest `755ea5ab90c1a03253c53128b63c1fb4026107fa76719362ff499bf45bbde083`.

`ontology-dialogue propose` called `http://127.0.0.1:8317/v1/chat/completions` with model id `grok-4.7` and the unread 6-byte token file. Propose exit 0. The model returned `SELECT COUNT(*) AS row_count FROM public.entry;`. A local approval card named that proposal. Execute exit 0.

The execution row is `{"kind":"int","value":"90000"}`. `citation_status` is `ok`. Query id `q_fecee3762b8b3c1d`. `authenticated` and `authenticated_audit` are false. `auto_execute` is false. `model_sql` is `untrusted`.

The provider body names model `grok-4.7-build`. Usage is prompt_tokens 2461, completion_tokens 256, total_tokens 2717, cached_tokens 1152. The body has no cost field. The saved response, the proposal, and the execution stdout do not contain the token bytes.

## Tests

Smoke docker exit 0. Focused attempt 1 compiled the product tests that were mounted: pack compat 3, reviewed-effective integration 5, role-pack 11, and builder clippy `-D warnings` exit 0. The lib golden failed because `examples/ontology-draft` was not mounted (35 passed, 1 failed). Dialogue tests failed the same way on the missing TLS fixtures. Focused-2 mounted those trees: lib 36, the same integration counts, dialogue execute_approval 3, identity_compat 5, propose 6, all exit 0. Its clippy failed because the component lives in the image layer. Focused-3 reinstalled clippy 0.1.94 and `ontology-dialogue --all-targets -D warnings` exited 0. No `cargo update`. Root `Cargo.toml` and `Cargo.lock` were not changed.

## Failed attempts kept

`linux/focused/HARNESS.txt` records the mount gap. `failed-attempts/pg-semicolon` records the `psql` statement join. `failed-attempts/pg-query-stdbool` records the missing compiler header. `failed-attempts/gateway-token-store-mount` records the unread token-store sources. a389 freeze and gold were not rewritten.
