# a086 fixed review — structure closure findings 3 and 4

Session `eef831aa-dba4-4af2-a78b-541bc9e0a086`. Reviewer grok-4.7, no subagents. The production tree was not edited. This file and `findings.json` are the review record. The coordinator commits them afterward.

## Decision

Approve these two a071 findings on commit `eebc0ea3b835655fdb3ab088e6f7351d0d316a43`:

- `effective-fk-names-rejected-column` (finding 3, high)
- `duplicate-key-member-accepted` (finding 4, medium)

The approval covers that scoped pair. It leaves SP07-W01, whole SP07, the vertical, security, runsc, customer delivery, protected CI, and release unsigned. a081's claim that the slice is closed is the author record. Acceptance here is this decision, limited to findings 3 and 4.

## Commit

| Field | Value |
| --- | --- |
| Commit | `eebc0ea3b835655fdb3ab088e6f7351d0d316a43` |
| Tree | `eebbcb4a8ff44d50c4b541d5646796a82a234210` |
| Parent | `79587e4bcc168b1cb2bfeee2690f62225ddb7dcd` |
| Prior reviewed compiler | `4c989bce58407cf19d3d93dc600162d7322d92f8` (a071) |
| Author Linux tree | `93c6cc57f1ad3dc6e7dcc67ba8761316405ced91` (911f317 overlay) |
| Archive | git archive of `eebc0ea3`, extracted under `/tmp/oo-a086-eef831aa-dba4-4af2-a78b-541bc9e0a086/snapshot` |
| Archive check | files=2903, missing=0, mismatch=0 against `git ls-tree` |
| Inventory | `msrv/archive-inputs.sha256`, 2903 lines |
| Workspace | not checked out onto this commit |

The five closure sources have the same git blob in `eebc0ea3` and in tree `93c6cc57`:

- `crates/oo-builder/src/draft.rs` `1789ac8f1e235d655c9352bae09e2c4783d05273`
- `crates/oo-builder/src/pgsql.rs` `5374e5e85e610d87df5178f32437625e76c19757`
- `crates/oo-builder/src/draft_tests.rs` `5a2db165a8423a5c67a6229525bf028fedf8d9f0`
- `crates/oo-builder/tests/structure_draft_cli.rs` `79983a58b65f3bd2e72f2334ffbe4f62dcc8c5c5`
- `contracts/builder/structure-draft/README.md` `8a8e1c8cc0c42cd343a0faff1f8ff527c1a946bf`

`nfc.rs` `957c4dc5fa5d1b846df694821eb3e33df985b6ef` and `nfc_tables.rs` `f98496d861b725389f8c1d771ef78f7cc5fa4762` are identical at a071, the parent, `eebc0ea3`, and tree `93c6cc57`. Content SHA-256 of the archived sources used by the probe: `draft.rs` `9633935006260fb34346558fe24beece3d8b74986b361817d63dc9178b34c324`, `pgsql.rs` `9ebaf44c9e77facf091463b5068b24edd582418c59a7da429353c087a1ca18eb`. Attempt-6 compared `/src` and `/work` and those two hashes matched.

## Linux smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. `RUSTUP_TOOLCHAIN=1.94.0` overrode the archive `rust-toolchain.toml` channel `1.98.1`. Observed compiler: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `cargo 1.94.0 (85eff7c80 2026-01-15)`. Pull never. Cap drop ALL. `no-new-privileges`. Own bridge. No host network, socket, HOME, prune, or shared cache. `OO_WRITE_DRAFT_GOLDENS` was not set.

Attempt-1 run `e2364472-4959-4f54-b815-2ac993ff51ff`, container `d7cc3af039190529ccb6fd6b58f5f4fd1b157dc2064137a1b15672f988ef4377`. Fetch, locked offline test, and clippy `-D warnings` exited 0. Counts: lib 27 passed, bin 0, integration 4 passed. Smoke binary SHA-256 `de688f114cb7c8819eb51e040d6f392d8168ae20cd5d1279468bbc18b67456f0`.

Two attempt-1 exit files stay as the failed checker and the failed probe, and were not rewritten:

- `cli.exit` is `1` (SHA-256 `4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865`). The checker omitted the `oo-builder: ` prefix. The saved stderr for all three duplicate fixtures is the product line `oo-builder: oo.builder.catalog-evidence/v1/duplicate.key_member at /catalog/tables/primary_key: repeated key member`, SHA-256 `fb0b971f68b7ad218a210a9595229a684b8d825de961ce967b78e906d57fa700`, stdout empty, process exit 1, leak tokens absent. `cli/prefix-recheck.txt` records that.
- `probe.exit` and `docker-run.exit` are `101` (SHA-256 `39b8dc3fc8b44765c8e6f1adee04c5b465e555ab791cc42d0d9e810d5b64297c`). The appended probe moved `column` in `add_column` (E0382) and had an unused `mut parent`. That is a probe compile error. Library sources were not edited.

Attempt-2 run `7c2e9a10-4b61-4e0a-9c3d-a086probe02` exited 3 before `docker run`. Running builders at that moment were `oo-a080-msrv-f669cc94` and `oo-a087-msrv-227a9269-f27d-4d99-93cd-e34b64f018d9`. No container, network, or volume was created. `deferred.txt` SHA-256 remains `e30c738b198fe8c3d072dbf0446d7ebb67595061864513096378f9589538a3d7`.

Attempts 3, 4, and 5 rechecked the same smoke binary and then stopped inside the independent probe. Their exit files and `cases.jsonl` are kept:

| Attempt | Run | Container | CLI | Probe | Docker | Stop |
| --- | --- | --- | --- | --- | --- | --- |
| 3 | `3de65218-d87b-428c-98f2-ab8cc17bc51d` | `30dc32d5087a768d83ee54bbafdb9b87a8d169f60c39951e9dd4dfc586d0c3b8` | 0 | 101 | 101 | Curated relationship carried `label`, which `deny_unknown_fields` rejects as `schema.invalid` before the review. Cases through `stale-relationship/removes-nothing` had passed. |
| 4 | `44c7e9cc-d2a8-4f0a-bd61-a272b209a792` | `a38ca50b998a395a2511a77054e05543d15cd44e89f1f505e5f823d5061112c9` | 0 | 101 | 101 | `bind()` panics on an empty unique key because `parse_catalog` returns `key.empty` before a schema hash exists. |
| 5 | `07078496-6a84-4a98-bf74-f9e141007877` | `4d8ba35ed8172379f94cb630304112e039f7e5dff0bdf65c82e3fe02a4f3f2ff` | 0 | 101 | 101 | Two columns, U+1E14 and U+0045 U+0304 U+0300, share ident `q_c492cc80`. `bind()` panics on `ident.collision`. The ident line was recorded. |

Attempt-6 run `e3577d45-8eca-430b-a12d-2863a12db668`, container `24c28a1a0b35d559c8662599c9f18490d102a3d6889611211e56d0ebbfc40740`, is the completed probe. Capacity before launch: builders=0, own=0, host cargo/rustc/rustdoc/rustup=0. It reused the attempt-1 cargo and target volumes and the attempt-1 network. It did not repeat fetch, test, or clippy. `rustc_vv.exit` 0, `binary.exit` 0, `cli.exit` 0, `probe.exit` 0, `docker-run.exit` 0. OOMKilled false. Privileged false. Memory 8589934592, swap equal, NanoCpus 4000000000, PidsLimit 4096, User 0:0, tmpfs `/tmp` 1g. Binary compare: both fields `de688f114cb7c8819eb51e040d6f392d8168ae20cd5d1279468bbc18b67456f0`. CLI summary: 11 goldens, 3 duplicate fixtures, 5 foreign-key fixtures, and the extra-arg check, all `ok`. Duplicate fixtures still exit 1 with empty stdout. Extra args exit 2. Probe: `draft::tests::a086_independent_counterexamples` ok, 1 passed, 27 filtered out, 0.49s. `cases.jsonl` has 118 lines, 0 failures. `summary.json` is `{"failures":0,"items":[],"ok":true}`. Probe source SHA-256 `2df541e2103d91b45a3743062a4d630837dad256bf427a94356ea2a0bf2b2d16`.

## Finding 3

`effective-fk-names-rejected-column` is closed for the scoped invariant. After compilation, every effective foreign key names a dataset and columns that are still effective. A fresh rejection removes only the relationships whose effective endpoints disappear. The queue kind is `relationship_endpoint_not_effective`, pending, `publishable` false. Generated relationships, the curated echo, and the historical decision stay. An accept on another item does not restore a rejected column and does not stale that rejection. A stale decision removes nothing.

Observed on attempt-6, all `ok: true`:

- Baseline effective names are `fk_alt`, `fk_bill`, and `fk_ship`.
- Rejecting shared source `public.orders/customer_id` leaves only `fk_ship`. `fk_bill` and `fk_alt` are `from_column_not_effective`. Generated still has three relationships. The curated column echo stays `rejected` with label `kept-label`.
- Rejecting both ends of bill and alt yields `from_and_to_column_not_effective` and leaves `fk_ship`. The customer primary key is stripped. The unrelated customer column remains.
- Rejecting every customer column yields `to_column_not_effective` on the dependent foreign keys, leaves the customer dataset shell, and keeps `fk_ship`.
- Accepting the dataset, accepting a sibling column, or overriding the label does not restore `customer_id` and does not drop the rejection.
- A later bound version that puts the column back restores `fk_bill` and `fk_alt`. The earlier rejection remains a stale decision. The curated status stays `rejected`.
- A stale relationship rejection leaves all three foreign keys and adds no endpoint item.
- A fresh human rejection of `fk_ship`, with curated `{"name":"fk_ship","status":"rejected"}`, leaves `fk_alt` and `fk_bill`, adds no `relationship_endpoint_not_effective` item, marks `candidate_relationship` rejected, keeps `fk_ship` in generated, and echoes curated status `rejected`. Rejecting `customer_id` afterward still adds no endpoint item on the already rejected ship.
- A dataset-level reject action returns `review.not_in_scope` and emits no draft. That behavior is the pre-existing review scope from a071. It is recorded below and does not reopen finding 3.
- A parent in a 31-byte uppercase schema is not emitted. The queue contains `identifier_not_in_oos`. Effective foreign keys are empty, so no effective foreign key names the missing dataset. `endpoint_dataset_not_effective` is not required for that input.
- An unverified referenced list (`fk_name` to `customer.name`, which is not the parent key) stays effective, status `draft`, with pending `fk_target_key_unverified` and `publishable` false. Four effective relationships remain. That emission stays an unsigned residual. This review does not treat it as an authorized key.
- Composite member order is preserved (`account_id`, `tenant_id` locally and `id`, `tenant_id` on the parent). Rejecting one referenced member closes the whole foreign key with `to_column_not_effective`.
- Self-reference from and to close only the affected end (`from_column_not_effective`, `to_column_not_effective`).
- An NFC `é` endpoint rejection closes with `to_column_not_effective`. A review item that uses the non-canonical name is `review.unknown_item`.

Every guarded draft in the probe kept publication `unavailable`, llm `not_used`, certification `none`, `pg_catalog_integration` `not_accepted`, every queue item `publishable` false, and every physical column `type_support.converted` false. The eight checked-in fixtures recomputed to their own catalog and binding hashes. The five foreign-key fixtures compiled with no effective-endpoint hole.

## Finding 4

`duplicate-key-member-accepted` is closed. A repeated member inside one primary key, unique key, or foreign-key source or referenced list is a static `oo.builder.catalog-evidence/v1/duplicate.key_member` after a hash the probe recomputed. The diagnostic text does not contain the canary name `a086_name_7c2e` or the canary comment. CLI stderr for the three checked-in duplicate fixtures is the static line above, process exit 1, stdout empty.

Observed on attempt-6:

- Repeated primary-key attnum, repeated primary-key name, same attnum with another name, same name with another attnum, NFC `é` versus `e` plus acute, unique-key repeat, foreign-key source repeat, foreign-key referenced attnum repeat, and foreign-key referenced NFC alias all return `duplicate.key_member`.
- An empty unique key and an empty foreign key return `key.empty` from catalog load, before a hash and before the duplicate check. The probe passes the unbound envelope to `compile_structure_draft` for those inputs.
- Attnums 0 and 1601 return `key.unknown_column` during load.
- An empty primary key beside a distinct unique key is legal. The effective primary key stays empty.
- Case-distinct `id` and `Id`, a distinct unique key, and two foreign keys that share a column stay legal and keep their order.
- A zeroed catalog `schema_hash` and a zeroed binding `schema_hash` return `hash.schema` before the duplicate error. Zeroed `config_hash`, `type_hash`, and `evidence_hash` return `hash.config`, `hash.type`, and `hash.evidence`. The canaries are absent from those messages.
- U+1E14 and U+0045 U+0304 U+0300 both become ident `q_c492cc80` (`same=true`). Two columns with those originals return `ident.collision` at load. A key that lists both spellings on columns that do not themselves collide (`U+1E14` and `other`) returns `duplicate.key_member`. Neither catalog is accepted. This is the inherited finding-1 composer miss. It is a rejection, so it is not a new duplicate-member hole. Findings 1, 2, and 5 stay open. `nfc.rs` and `nfc_tables.rs` are unchanged from a071.

## Governance

Folded from `governance-notes.txt`. The functional decision above is taken from the archived commit. The live connectors interface stays unaccepted until an independent a083 rebuild and review. a081 names a073 as the dependency owner. The live owner is a083.

The a081 session record shows `cp -p` swaps of `crates/oo-connectors/Cargo.toml` at 2026-10-02T01:30:36Z (`call-2741e0c6`) and 01:30:48Z (`call-69da0bbc`), then root `Cargo.toml` and the connectors manifest at 01:33:20Z (`call-ee336cde`) and 01:33:29Z (`call-c4194dfb`), each restored with `cp -p`. No hash of the saved temp file was stored. The 01:30:48Z check compared the working tree with HEAD and then refreshed the temp path, which discarded the first snapshot. The session text said a replace would happen only when the file still matched HEAD. The executed commands did not do that compare. A trap restore can overwrite a concurrent writer.

Git blob `d10c596f2183f0305341afde191fc7561ad4750d` is `crates/oo-connectors/Cargo.toml` at `8370498`, `911f317`, parent `79587e4b`, and `eebc0ea3`. Root `Cargo.toml` blob `1dd71ee45257f548346d1a221dee280466546558` is the same at those commits. That identifies the bytes `git show HEAD` would have written in. It does not identify the bytes that were restored. Absent: a pre-swap SHA-256, a byte compare of the temp snapshot with the restored file, and the temp files themselves. The current connectors SHA-256 `b9d8c7bfda4c563994024190ab0ad73c9fffefe013625a760aa47bc1f483216d` matches the coordinator dispatch record. It is not proof of the bytes at 01:30Z. This review did not modify live a083 files.

## Left open

- a071 findings 1, 2, and 5: `nfc-composition-equal-class`, `unicode-data-notice-missing`, `nfc-unicode-17-combining-classes`.
- Unsigned numeric scale, including PostgreSQL's signed scale.
- Physical identifier length versus PostgreSQL's 63-byte mapping. This slice emits no DDL.
- Unverified foreign-key semantics. The compiler still emits the relationship and queues `fk_target_key_unverified`.
- Real PostgreSQL, RBAC, profile, PII, LLM, publication, and query.
- The connectors interface and any live manifest drift. See Governance.
- A whole-dataset `review.not_in_scope` action. No draft is emitted, and the 31-byte schema case emits no dangling effective foreign key.

## Cleanup

Owned resources were labeled `oo.owner=a086` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a086`, name prefix `oo-a086-`. Host `cargo`, `rustc`, `rustdoc`, and `rustup` process count was 0. Every owned container was already exited. Labels were inspected again immediately before each removal. `cleanup.txt` records the removals at 2026-10-02T10:55:43Z through 10:55:45Z. Each `docker rm` exited 0. Owned containers, networks, and volumes remaining: 0. `cleanup_fail=0`. The image was left in place. No prune. Foreign containers, networks, volumes, the archive under `/tmp`, and historical exit files were left in place.

Containers:

- `24c28a1a0b35d559c8662599c9f18490d102a3d6889611211e56d0ebbfc40740` attempt-6, exited 0
- `4d8ba35ed8172379f94cb630304112e039f7e5dff0bdf65c82e3fe02a4f3f2ff` attempt-5, exited 101
- `a38ca50b998a395a2511a77054e05543d15cd44e89f1f505e5f823d5061112c9` attempt-4, exited 101
- `30dc32d5087a768d83ee54bbafdb9b87a8d169f60c39951e9dd4dfc586d0c3b8` attempt-3, exited 101
- `d7cc3af039190529ccb6fd6b58f5f4fd1b157dc2064137a1b15672f988ef4377` attempt-1, exited 101

Network `15111804e05b9d49c40bebe3cf2c94f32a4a51fb9553ee376b0d7f35dcf31e48`, name `oo-a086-e2364472-4959-4f54-b815-2ac993ff51ff-net`.

Volumes `oo-a086-e2364472-4959-4f54-b815-2ac993ff51ff-cargo` and `oo-a086-e2364472-4959-4f54-b815-2ac993ff51ff-target`.
