# Review 4 — scope and containment

Scope: owned paths, forbidden paths, builder limits, and secret markers. No builder for this review. The inventory builder is a later development step and is not part of this verdict.

## Checks

After commits `83b4f94a` and `e447249`, `git diff c828e2781fc37eb62249fab54fa0b7fff86f0249 -- crates/oo-gateway contracts/gateway Cargo.toml Cargo.lock` is the new contract file and three insertions in `reviewed_policy.rs`. The new test is the commit that added `crates/oo-gateway/tests/reviewed_policy_quantity.rs`. `lib.rs`, `policy.rs`, `src/bin/reviewed-policy.rs`, the previous gateway tests, and the lock were not in that diff.

Hostconfigs for the successful smoke, the successful post-edit, and the chain record `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, `NetworkMode=oo-a254-eef831aa-net`, `OOMKilled=false`. Failed attempts use the same limits and are preserved with their consoles and diagnoses.

A search of this evidence tree for `-----BEGIN `, `PRIVATE KEY`, `password=`, a `postgres://` URL with a password, and `api_key` or `api-key` returned no matches. No `*.key`, `*.pem`, `*.pass`, or `id_rsa*` file is present. `pg/pg-wrap.sh` sets `password_encryption=scram-sha-256`. Request JSON refers to `password_file` paths under `/secrets/`, which are outside this tree.

`overlay/reviewed_policy.rs` and `overlay/reviewed_policy_quantity.rs` compared equal to the worktree files before those commits. The chain binary identity phase matched the post-edit reviewed-policy hash `2581ee8b29d923a3421f88e20d45cb06fb1cda18cbfea1d6cabe24c70e6cbd72`.

## Verdict

The product change stays inside the frozen cell. Containment on the recorded runs matches the session limits. Secret bodies were not written into this directory. No scope expansion from this review.
