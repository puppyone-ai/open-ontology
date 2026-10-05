# Report

Session `eef831aa-dba4-4af2-a78b-541bc9e0a217`. Unit M1-D18A / SP07-W05. This report records the Linux proof. It does not accept the unit, close M1-D18, or release.

## Archive

Fixed commit `c2db9c430b822a824a1062b6c3c7345924df7dd8`, tree `8fb187147b02d6cacf45cb5a683859f698b497cf`. Product commit `9daeaa05e34c154391a757fc78fb47f09c954c6a` on parent `02b84f4bf731f6a24dfc6aece454d7e875d86938`. The green build is the fixed archive plus the five owned files. It is not a build of the parent tree.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `--pull never`. Memory 6442450944, NanoCpus 1000000000, cap-drop ALL, no new privileges, private network `oo-a217-eef831aa-net`, volumes `oo-a217-eef831aa-cargo` and `oo-a217-eef831aa-target`. No PostgreSQL. No host network, socket, or host home. `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

## Pre-edit smoke

`linux/smoke-attempt1` is docker-run exit 101. `captured_pg` passed. `review_pg_cli` could not read the two catalog fixtures because the partial archive omitted them. The container was removed. `OOMKilled=false`.

`linux/smoke` is attempt 2. docker-run exit 0. Container `ea86511b5b070abade03843fd2505a8e945d6a9a3a24d08f429c5e095409ccd0`, removed. The archive added `catalog-before.json` and `catalog-after.json` from the fixed commit. Overlay absent. Results: `captured_pg` 6, `review_pg_cli` 4, `role_pack` 11, `role_pack_cli` 4. Lock unchanged.

## Overlay

`linux/overlay-attempt1` is docker-run exit 101. Container `4f1dcebe96394fcc878afc8308772e1350ff63ecd6066e4650b74bd2b9589ee5`, removed. Compile error `E0432`: `CivilDate` is `oo_core::oos::CivilDate`.

`linux/overlay-attempt2` is docker-run exit 101. Library tests passed, 4. The integration test expected a rejected `email` column to stay on the effective dataset. `merge_effective` removes `status: rejected` from the effective list, and the compiler returned `decision.unknown`. The contract and the fixture were corrected. The product bytes of that attempt are `linux/overlay-attempt2/product`.

`linux/overlay` is the green run. Container `97cd53d005ba429a86563ed6276ec1f4f3091c0d7d8023dffa5a377681e7e0be`, removed. docker-run exit 0, overlay.exit 0, `OOMKilled=false`. Phases sha256-tool, owned-hash, clippy-component, fetch, lib-test, cli-test, clippy, and lock-hash are 0.

Library `reviewed_effective`: 5 passed. Integration `reviewed_effective`: 5 passed. Clippy `-D warnings` on `--lib --bins --test reviewed_effective` finished in 16.84s with no warning. The compiled bytes match commit `9daeaa05e34c154391a757fc78fb47f09c954c6a`.

Commands:

```text
cargo test --locked --offline -p oo-builder --lib reviewed_effective -- --test-threads=1
cargo test --locked --offline -p oo-builder --test reviewed_effective -- --test-threads=1
cargo clippy --locked --offline -p oo-builder --lib --bins --test reviewed_effective -- -D warnings
```

## What the green fixture did

`oo-builder review-pg-draft` compiled the locked shop catalog (`schema_hash` `bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c`). `reviewed-effective-oos` applied explicit decisions for `tenant_id`, `id`, and `customer_id`. `email` stayed null and hidden as `classification.absent`. `oo-builder role-pack` then built `oo_r_clerk` (relationship `purchase_customer_fk`, source-declared, not a grant) and `oo_r_peek` (no relationship). `oo_r_ledger` omitted `customer_id`, and the relationship text was absent. Allowing `email` returned `classification.unknown_pii`.

Stale fact, unknown column `contact`, duplicate decision, invalid PII `secret`, and a renamed catalog with the old schema hash stayed refused. Curated `pii: personal` was not copied. Money without a unit was `oos.invalid`. A structural reject of `email` removed it from the effective draft; a decision naming it was `decision.unknown`, and a role pack naming it was `visibility.unknown_column`.

Provenance fields `authentication`, `authorization`, `grants`, `publication`, and `semantic_publication` are `not_claimed`. The role-pack manifest kept `authentication` and `publication` as `not_claimed` and `row_security_reproven` false.
