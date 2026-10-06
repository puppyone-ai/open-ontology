# Review 1 — adapter cell

Scope: frozen `compatible()` for int2, int4, and int8 quantity. No builder for this note. The proof builder is `linux/run`.

## Checks

`linux/run/integer-arm.txt` is the archive arm from `"int2" | "int4" | "int8" => matches!` through the following `"text" =>`. It lists `SemanticType::Quantity` with identifier, foreign key, code, and status code. It does not list `SemanticType::Money`. Git blob of `reviewed_policy.rs` at `e447249` is `95e68417e6fe1be024123e3aa2b7c2301402eb21`, matching `source/blobs/reviewed_policy.rs`.

`linux/run/phases/test-reviewed-policy.exit` is 0 (21 passed). `test-reviewed-policy-quantity.exit` is 0 (6 passed). `clippy-product.exit` is 0. The overlay copy of `reviewed_policy.rs` compared equal to the archive (`overlay-unmodified.exit` 0).

The independent harness kept quantity on int2, int4, and int8 and refused the wrong physical types, the missing and stale decisions, money at `reviewed-effective-oos`, and the other named semantics. `classify/classify.txt` is `classify=ok`.

## Verdict

APPROVE_LIMITED. The frozen integer arm matches the quantity contract. Decision, seal, bool, text, and money behavior stayed in place on this run. This note does not accept the parent adapter or any later semantic type.
