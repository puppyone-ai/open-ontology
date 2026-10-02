# a095 handoff

Local commit for independent review. No push, no pull request, no release, no self-approval.

The parent of this commit is `1b9110265aae92bdd7eb02a71dcbeacf648375a5`, tree `94c10a1b333608f4000913c681229577de7b732b`. That commit's parent is `77abcc1731a91cb9933847133e7ae354d289fd46`, tree `47f58c94372c293e1512f220a7fe7e87a58a03ac`. Task baseline `7143b2d9a7888d242983e5b9c10ef597246d5848` is an ancestor. This file cannot record the hash of the commit that contains it. Read `git rev-parse HEAD` and `git rev-parse 'HEAD^{tree}'` after the commit.

The Linux archive was `77abcc1731a91cb9933847133e7ae354d289fd46` plus the owned overlay. `1b911026` changes four documentation paths only (`execution/decisions/vertical-integration-order.md`, `execution/prompts/a097-evidence-retention-followup.md`, `specs/open-ontology-development-ledger.md`, `specs/open-ontology-v0.1-plan.md`). It does not change the lock, the manifests, or `oo-cli` / `oo-sdk` / `oo-core`.

SDK interface stayed `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df` (`sdk: scrub accepted secrets and stop overlong Retry-After`). `git diff` from that commit to the parent for `crates/oo-sdk` and `crates/oo-core/src/api.rs` is empty. Original SDK commit `8efeb230afc6734a2e50eaca3f09fd78300a9c7d`. a090 approved the six runtime findings and left the CLI unsigned. a084 had requested changes on the original SDK; a089 is the correction a090 accepted. Those evidence trees were not edited.

## Owned paths

- `crates/oo-cli` source, manifest, and tests
- `contracts/cli/query` README and fixtures
- `Cargo.lock` `oo-cli` dependency names only
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a095/SP09/cli-query/`

Root `Cargo.toml` was not modified. `oo-connectors`, the SDK, core, builder, vendor, jobs, and CI were not modified. No foreign evidence was staged.

## Protocol

`POST /v1/query`, `Content-Type` and `Accept` `application/json`, user agent `oo-sdk/0.1`, bearer from the explicit source only. Body is SDK canonical JSON, nulls omitted, no `client_context`. Markdown is rendered after the typed door. `--json` prints that typed value again, plus one newline.

## Source hashes before this evidence text

| Path | sha256 |
| --- | --- |
| `crates/oo-cli/Cargo.toml` | `3dc91f91af95a4fbb61f3fb00a8220129d17d9b4658332411103972910e82a28` |
| `crates/oo-cli/src/lib.rs` | `9bf2f55049123a093a0d3775100a59e03998f1f5014a148fc35ff2a79e074a92` |
| `crates/oo-cli/src/bin/oo.rs` | `2f350a5427d2489c49b01cd494d2168804f6b0631f8fbc3089ab3023ee62c582` |
| `crates/oo-cli/src/args.rs` | `1c3cf685aa6e023dd3c2308092395808e0aedc8c0da54960d7248f8ace17328e` |
| `crates/oo-cli/src/load.rs` | `bea398dff3accd627bad42d64397f3574768decbfb1fef77b71d429a203cfbb6` |
| `crates/oo-cli/src/outcome.rs` | `74377748b8efddfe6e4cac8007f44d4f2dbcba77f41bd2b8e2c838157722ec41` |
| `crates/oo-cli/src/render.rs` | `47a216e3ca0e81ac7dd85852b14115af83bcc5512af18a2a94ffaff80e9281f0` |
| `crates/oo-cli/src/run.rs` | `58226e5bc0cfcedbadb091535b6d9e774cf354f9220439e8d9c9d81e503d8e15` |
| `crates/oo-cli/tests/query.rs` | `0208153708fe420fb625519451915d34ae3195496e8f83371868024b89a8d087` |
| `crates/oo-cli/tests/common/mod.rs` | `2291b070c5cbee742c7f0436c06c127cb943eccd5438085ec0140e697c35eddf` |
| `contracts/cli/query/README.md` | `aee8694eeb1ea112e0e95c669b30efe1ca373658e9cfb1df069fb2fcae320145` |
| `Cargo.lock` raw | `40664b41001d01db59d272e0d9fab3977842becc88998bd52eefaa0c1b7a7147` |
| `Cargo.lock` git blob | `887cef930ba2fdadf3541aded755dc594e5bd1d7` |

## Linux proof

Run `6fc77428-7794-4ccf-b153-1a0194fa8d82`. Image digest `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Commands and exits are in `REPORT.md` and `linux-194/out/phases/*.exit`. Fetch, test, clippy, and docker run are 0. Nine integration tests passed. Cleanup of that UUID is 0 and the snapshot directory was removed.

## Not signed off

Dual architecture, a real Gateway, PostgreSQL, conversation, sandbox proxy, management, config validate, SQL validate, export, discovery, other commands, and the rest of SP09-W02 remain pending. Stop here for an independent fixed review.
