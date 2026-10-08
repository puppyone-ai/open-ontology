# HANDOFF

Reviewer session `eef831aa-dba4-4af2-a78b-541bc9e0a404`. Verdict: **WITHHOLD**.

Owned writes: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a404/SP08/release-control-api-review/**` only. No product commit, no ledger edit, no plan checkbox, no push, no release, no customer call.

## Acceptance boundary

`full_sp08_accepted` is false on `linux/proof/private-pg.json` and on the reader view in `linux/csrf/observe.json`. SP08 is not accepted. This card is not accepted. The parent goal is not accepted. `customer_approval` is `not_issued`. This session is the independent review. It is not a production signature.

## What was executed

One private PostgreSQL `oo-a404-pg` and one Rust 1.94 builder `oo-a404-build`. Smoke, locked offline Clippy, the CSRF probe, the product tests, and the post-test observe all exited 0. Raw lines are `linux/raw/raw-exits.txt`. Docker exit is `linux/build-1/docker-run.exit`.

Pins, from `linux/archive/trees.txt`:

- Product `fbd0467617a3c1514e7ffbef52f1138e20416ac6` `tools/release-control-api` tree `7375587fb4758d41ef02727451daf3473c29cac4` (27 files). Contract blob `dc64a53f5f54b8f4b080ecaa8561a16605d65d34`.
- Identity `a6f92da8a0bf872f8a30efb116c2c7b14463505b` `tools/control-identity` tree `45f330af6e326d8f1cc1038ef47de21031ef751a` (28 files). Host worktree dirt is `linux/build-1/identity-worktree.txt`. Those files were not compiled. 402 remains the only identity writer.
- Producer `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3` `tools/release-coordinator` tree `5a3692de6f1e5609d684164a34af672b11c163b6` (16 files).
- `crates/oo-builder` at the product commit, tree `40f74d7e9aac9c13e95d1da3d61d689184773c77` (41 files). Later `HEAD` commits that change `oo-builder` were not compiled.
- Worktree `HEAD` when the archive was taken: `18b0fcde61ea269cc27e2dc34a81c07ba413a30e`.
- Successor `e2bd526e96175efb52fa1181843fbe991ae7992b` landed after that archive. It was not a second builder and it is not accepted.

`pg-wrap.sh` hash `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`. Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.

## Fixes still required

1. Remove `POST /control/v1/auth/bootstrap/consume` from the CSRF check. Do not amend the contract to bless that probe or to accept a 43-character header that does not match the stored hash. Identity must add a no-write confirm. Until that route is pinned, release mutations fail closed. The consume path commits `bootstrap_rejected` on a CSRF-valid call. This run counted 16 such rows on `oo_a404` and one on `oo_a404_csrf`.
2. Bind `getActiveRolePack` and `publication_status` to pack membership. `WHERE singleton` and `candidate_file WHERE candidate_id = $1` do not. The uncompiled `READ_BINDINGS` select has no `WHERE`, and its role check is a management role, not `oo_r_*`. `project.admin` and `audit.read` are not membership in the sealed pack. Do not stamp `project_id` from process config when the reader SQL omits it.
3. Do not sign production. `test_issuer`, missing owner, missing runsc, and a fence or replica that is not production-admissible stay failed. No real customer gate was in this run.

## Machines after the review

Owned containers, volumes, the network, and `/tmp/oo-a404-eef831aa-dba4-4af2-a78b-541bc9e0a404` are removed by the cleanup recorded in `CLEANUP.md`. Images stay. Foreign containers were not removed. Replaying the proof needs a new private database.
