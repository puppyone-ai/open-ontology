# Benchmark tools review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a242`. Fixed archive `4b15249b70ee38d5a4d21c546b1329e6532ccfed`, tree `31c428af2494756e2ea569f8c3377c04bc706907`. Repository HEAD recorded at launch is `64153894d3b0b0e184202e7b6054d743578f71ed` in `linux/proof/repo-head.txt`. The proof built the archive, not that HEAD.

This review covers three existing units. It does not change `specs/open-ontology-acceptance-counting.md`. Published denominators stay where that file left them. B-D04, the SQL runner, a full agent, a customer benchmark, and a release are outside this review.

## Verdicts

| Unit | Authored at | Verdict |
| --- | --- | --- |
| B-D01 result comparator | `0ae42f942e075be4171646d373c49dcfe1029817` | APPROVE_LIMITED |
| B-D02 query-result adapter | `ed661809cf6c1e8df37af5842838dd1eb839c98a` | APPROVE_LIMITED |
| B-D03 trial statistics | `4b15249b70ee38d5a4d21c546b1329e6532ccfed` | APPROVE_LIMITED |

B-D01 is an exact typed comparison. The fresh cases covered null, missing, decimal rescale, decimal value change, negative scale, zero coefficients at two scales, ordered swap, unordered swap, unordered multiplicity, text value change, and partial versus complete in both directions. Report bytes matched the handwritten strings. Help and usage matched the frozen CLI text.

B-D02 maps one query-pg JSONL stream to one typed actual. Text `1.20` became coefficient `120` scale 2 and matched comparator gold coefficient `12` scale 1. `-0.00` exited 2 with `query-result-adapter: decimal /1/cells/0`. SQL null and an ordinal null became presence null and presence missing. A gold document was refused and the canary was absent from stderr. On the private PostgreSQL chain, the adapter output matched the handwritten three-row desk gold, then the comparator matched that gold.

B-D02 limit: a summary of `succeeded` with `truncated` true and `end_audit_ok` true is completion `complete`. The word `truncated` is absent from the adapter stdout. Against the full three-row gold the comparator exited 1 with a length difference, actual 1 expected 3. Against a one-row prefix gold the comparator exited 0. `linux/proof/harness/truncation-boundary.txt` records that pair and `runner_approved=no`. A runner must refuse that truncated result on its own, including when a prefix gold would match. This review does not approve a runner. The fixed archive has no `benchmarks/sql-runner`.

B-D03 is pass^k over full question vectors and one paired question bootstrap. The independent SplitMix64 oracle matched seed 0 draws `e220a8397b1dcdaf`, `6e789e6aa1b965f4`, `06c45d188009454f`, and `f88bb8a8724c81ec`. For n=2 the draws `u64::MAX` and `u64::MAX-1` are rejected. For n=1 every accepted index is 0 and `u64::MAX` is rejected. A k=3 vector with one question, left pass/pass/pass and right pass/pass/fail, produced point difference numerator -1 denominator 1. The report contains no `"denominator":3`. A second run was byte-identical. A paired two-question bootstrap at seed 7 and 4 resamples matched the independent oracle, including the index trace in `linux/proof/harness/bootstrap-indexes.txt`. A `needs_reannotation` trial kept `question_count` 2 and `reannotation_count` 1, failed the gate, and exited 1. A short right-hand trial exited 2 at `trial-statistics: trials /variants/1/questions/1/trials`. A project mismatch exited 2 at `trial-statistics: tuple /variants/1/tuple/project`.

B-D03 limit: this is one small enumerable oracle, not a question bank and not a customer benchmark. pass^k here is 1 only when every trial on a question is pass.

## Fixed source

The three crates are standalone workspaces in the archive. They are not root workspace members. Root `Cargo.toml`, root `Cargo.lock`, and product sources were not edited. `a207` remains the only connector-manifest and root-lock writer. The successful proof compared product bytes before and after; `product-after.exit` is 0 and the digests match `linux/smoke/product-sha256.txt`.

`source/blobs/` is a `git archive` of the three crate trees at the fixed commit. Recipe origins and hashes are in `ORIGIN.md`.

## Pre-overlay smoke

`linux/smoke/docker-run.exit` is 0. Container `459ed7c9c19646d6c3be8e723eb4eb8936e3711566a207f1f0062719f768736b`. `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. The smoke archive had no harness and no `benchmarks/sql-runner`. `console.log` is `smoke_ok`.

Author tests on Rust 1.94.0, locked and offline after the warm fetch:

| Crate | Passed targets |
| --- | --- |
| result comparator | 16 bounds, 4 CLI, 12 gold, 3 SHA-256 |
| query-result adapter | 11 adapt, 3 CLI, 1 stdout gold, 3 SHA-256 |
| trial statistics | 7 bounds, 3 CLI, 3 enumerate, 4 gold |

`linux/smoke/fetch-comparator-offline.exit` is 101. The following online `cargo fetch --locked` exited 0. Adapter and trial offline fetches exited 0.

## Decisive verification

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One CPU, 6 GiB, cap-drop ALL, no new privileges, private network `oo-a242-eef831aa-net`, private cargo volume `oo-a242-eef831aa-cargo`, private target volume `oo-a242-eef831aa-target`. `HOME=/tmp`. No host network, no published socket, no shared cargo cache.

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a242-pg` id `92b55633ef27e7e2c37de9cf9e8a37a409d99c186ecafd0e17eabaf40e894e53`. Memory 1 GiB, one CPU, cap-drop ALL, cap-add CHOWN, DAC_OVERRIDE, FOWNER, SETUID, SETGID. `ports.json` is `{}`. Aliases `oo-a242-pg` and `oo-a198-pg`. The frozen live test dials `oo-a198-pg`. The review request dials `oo-a242-pg`. Secrets stayed under `/tmp/oo-a242-eef831aa-dba4-4af2-a78b-541bc9e0a242/secrets` mode 0700. Passwords were lowercase hex files. No `PGPASSWORD`.

Successful proof container `1989f683e955f8b3d6e0e2fc0540963d5288de09a2b6e78328800c1ed31733e3`. `linux/proof/docker-run.exit` is 0. `console.log` is `proof_ok`. `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. The container was removed. Capacity before the run was `own=0 global=0 host_cargo=0`.

`rustup component add clippy --toolchain 1.94.0` exited 0. `cargo clippy --locked --offline --all-targets -- -D warnings` exited 0 for the comparator, the adapter, and trial statistics. The same clippy invocation exited 0 for the review harness. `cargo build --locked --offline` exited 0 for all three binaries and for `a242-review`.

`OO_A198_LIVE=1 cargo test -p oo-gateway --test query_pg_cli -- --exact live_query_pg_against_private_postgres` exited 0 in 2.12s (`linux/proof/live/fixture.exit`). That test provisions the frozen fixture. It is not a verdict for these three units.

The review harness then ran 44 handwritten cases, exit 0 (`linux/proof/harness.stdout`). The oracle does not call the three libraries. The live chain is compiled `query-pg`, then the adapter, then the comparator, against handwritten desk gold for lane, rank, stamp, note, and open. Ordered reversal mismatched. Unordered reversal matched. Replacing `beta` with `gamma` produced a value mismatch. Rewriting status `succeeded` to `partial` kept completion `partial` and mismatched the complete gold. `end_audit_ok` false exited 2 at `query-result-adapter: completion /summary/end_audit_ok`. A missing summary exited 2 at `query-result-adapter: protocol /`. The secret scan exited as case `secret-scan` and did not print secret bytes.

## Preserved failures

| Attempt | docker-run | Container | What failed |
| --- | --- | --- | --- |
| `linux/proof/attempts/1` | 1 | `16540acf5d8f80a510a7b3db449344b5317d447877deecbe36e78cd455a19ef1` | Image toolchain 1.94.0 has no rustfmt component. Rustfmt is not part of the a200 recipe. Removed from the proof. |
| `linux/proof/attempts/2` | 101 | `e18af5f35d98b777abc7dee619cb8aaf711a1905904892803d857169e5fe9cbb` | Harness clippy `-D warnings`: unused `mut`, unused `LeakStr` because `String::leak` is inherent, `useless_format`, `manual_contains`. |
| `linux/proof/attempts/3` | 127 | `94608d0e90bcfda60da3a9daa97c6f2274eaead085cc653aa0880e56603ea459` | `cargo clippy` did not leave `a242-review` on disk. `env` exit 127. The fixture test had already exited 0. The proof now runs `cargo build` and checks the binary. |
| `linux/proof/attempts/4` | 1 | `77154c79906046c1319eff8089095c81b184ab1f8648365ea2e686cd9956d821` | Secret scan matched the bytes `PRIVATE KEY` inside `attempts/1/review.rs.before-rustfmt`, which is a copy of the scanner source. The truncation boundary file from that run already recorded the adapter/comparator pair. The walk now skips the `attempts` directory. |

Each attempt left `OOMKilled=false`. PostgreSQL stayed up across the reruns. `pg/up.exit` was not overwritten.

## Limits

The comparator approves exact typed rows, not prose and not a score. The adapter approves one typed actual, and the truncation case above stays a runner obligation. Trial statistics approves the small oracle above. Acceptance counts in the spec file were not edited. No parent SP12 acceptance is claimed.

## Reproduce

Pinned images were already local. The proof does not pull. From the repository root, with smoke `docker-run.exit` equal to 0 and no `linux/proof/console.log`:

`bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a242/SP12/benchmark-tools-review/linux/proof/launch.sh`

That starts `oo-a242-pg` when `pg/up.exit` is absent, then one 6 GiB builder. Successful completion writes `proof_ok` and removes the builder. Own cleanup is `cleanup/cleanup.sh`.
