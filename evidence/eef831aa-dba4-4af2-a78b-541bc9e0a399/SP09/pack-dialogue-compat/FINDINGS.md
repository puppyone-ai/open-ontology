# Findings

The closed loop needed three agreements that the previous pins did not share.

The role-pack and reviewed-effective compilers accepted a 1..=63 slug and rejected a 64-hex policy id. Gateway `query-pg` requires `identity.policy_id` to equal the config digest. Dialogue already required 64 lowercase hex. The compilers now accept the slug or that digest and do not rewrite either form.

Dialogue knowledge slugs reject a hyphen, so `know-a317` fails in `parse_config` before a socket and before the bearer check. The compiler slug still allows the hyphen. The live pack uses `know_a317`, which both sides accept. No sealed a389 bundle was rewritten.

Production HTTPS and the 16-byte bearer minimum are unchanged. A 6-byte token is legal only when `endpoint.transport` is `development_local` and the origin is `http://127.0.0.1:<port>`. The pg-builder local reader was not modified.

The live model call succeeded. Provider failure still writes no proposal: `identity_compat` covers HTTP 500 with an empty stdout and no approval file, and this loop writes the approval only after propose exit 0. Execute ran once, after that card, and returned 90000 with citation status ok.

The provider response did not include a monetary cost. It did include prompt, completion, total, and cached token counts. The response `model` field was `grok-4.7-build` while the request `model_id` was `grok-4.7`.

`contracts/builder/role-pack-v1.md` and `contracts/builder/reviewed-effective-oos-v1.md` still describe the slug-only policy id. They are outside this ownership boundary.
