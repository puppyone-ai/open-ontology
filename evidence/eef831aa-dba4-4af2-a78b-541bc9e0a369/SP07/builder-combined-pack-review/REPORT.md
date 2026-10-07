# Combined pack review

Verdict: **APPROVE_LIMITED** of original E47. `parent_acceptance` is false.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a369`. Reviewed archive `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba`, resolved with `git rev-parse --verify` before `git archive`. Consumer baseline `a13b1cc80ed66c07199a41d47b31fd42530f2be9`. Producer `c23db28dc27f5cf20741dd36e8a8e88129ad1715` stays at a348 `APPROVE_LIMITED` for the column-field seam. a328 baseline `8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8`. This directory edits no product file, lock, root manifest, spec, or ledger. It adds no countable unit and no frontend.

The a357 withhold was that a per-table pack drops the other table's approved overlay and that no shipped command emitted one combined final role pack. That clause is closed on the stored shop documents and on an independent three-table fixture. The complete E47 card is the scope of this verdict. SP07, public authentication, semantic publication, release, and parent acceptance stay open. The E47 ledger checkbox is untouched.

## Decisive Linux run

`linux/proof-2`. Container `oo-a369-proof2`, id `6de98d9966c4ef2142063edf3ebd36bb6157e1da1b2177b8af9031ce7e2b4bcc`. `docker-run.exit` is 0. Console ends `PROOF_OK`. The launch script removed the container after `hostconfig.txt`.

HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a369-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, PidsLimit 256, ReadonlyRootfs false. `capacity.txt` before start: `own=0 global=1 host_cargo=0 cap_global=10 cap_own=1 postgres=0`. The one global builder was left running. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux arch=arm64`. No pull. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0` `(85eff7c80 2026-01-15)`. Kernel `Linux 6de98d9966c4 6.12.76-linuxkit #1 SMP Fri Apr 17 14:56:37 UTC 2026 aarch64 GNU/Linux`. `cargo-update-invoked.txt` is `no`. `postgres.txt` is `not_started`. `toolchain-aside.txt` is `moved-aside`, so the image stayed on 1.94.0 while the repo channel file was out of the work tree. Receipts mounted read-only at `/receipts`. `A362_RECEIPTS` and `A369_RECEIPTS` were both `/receipts`.

Phase exits, all 0: toolchain, receipts, archive-locks, fetch-builder, fetch-pack, fetch-builder-offline, fetch-pack-offline, pre-oracle, test-builder-combined, test-pack-combined, clippy-component, clippy-builder, clippy-pack, test-oracle, clippy-oracle, product-bytes, locks-unchanged, provenance.

Pre-oracle, with `overlay/a369_review.rs` absent from the archive:

- `pg-builder` `--test combined`: 9 passed, 0 failed, 5.06s
- `description-pack` `--test combined`: 6 passed, 0 failed, 0.00s
- clippy `-D warnings` `--all-targets` on both crates: exit 0

Independent oracle `tests/a369_review.rs`, content SHA-256 `b025de64603a5ba899b7fd61cfe160133b14ad17a46ea2f338ff923e57e05df3`, then removed before the byte compare:

- 3 passed, 0 failed, 7.57s
- clippy `-D warnings --test a369_review`: exit 0

Clippy logs contain no `warning:` or `error:` lines. `contract.rs` and `description-pack` `tests/pack.rs` were not executed. Their git blobs match `a13b1cc`. `testing/real-db/pg-builder` tree `17668deacecdd51215352f10cb9f828a9dbb2f7e` matches `a13b1cc` and the archived commit. Root lock content SHA-256 `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. Child locks `tools/pg-builder/Cargo.lock` `eeed52b1cf0f5b7aff7faa223ffda09f12c02bbdeb32e29a045e5b30a1fc27ca` and `tools/description-pack/Cargo.lock` `2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d`. Before and after copies match. `product-bytes.exit` is 0. HEAD `e1d77d75e1c7a4a9698cbdf6a8f5ea671dc4ca7e` has an empty diff against the archive for `tools/pg-builder` and `tools/description-pack`. The archive is still the full repair SHA.

a357 oracle content SHA-256 reconfirmed `b29bbd5e85ed7006c02dab912bb65b7c6a092f71cfc412a16d0aa51cbb1512af`. Provenance hashes of the mounted receipts matched before and after the run.

## Stored combined pack

`pack_combined` on the read-only a328 pass-1 generations, curated file, operator review, settings, classifications, and roles. `linux/proof-2/oracle-summary.json`:

- protocol `oo.pg-builder.pack-combined/v1`
- outcome `packed`, `reseal_count` 1, product `statements.reseal` `compiler`
- `original_wrapper_hash` `dca83e58b2a79efef34baf18f126d126f0e8c5f7a117c073b4343c2e40526d2e`
- `resealed_wrapper_hash` `b9f3f540eb288b18d948a2d53db22d4c2b42f342cfb648da94902860f9be3c52`
- narrow role contains both approved bodies; each per-table pack lacks the other; the combined role text is not their concatenation
- `authenticated` false, `parent_auth` `required`, `publication` `unavailable`, `semantic_publication` `not_claimed`
- party rebind `bound` true
- review hashes: party `14d23dd3a586d63570a58662ab11ff0843511081860d831700ad4c9a694bf5bd`, account `a6b82bebd1cef066e1c3175c17f3e9938863b6689a024dd1d7a6cd71fa649eb3`, entry `064ef52d5332a60287ddc0d7851472a575e478d697f6eed4ff43b874d9b015f0`
- prompt tokens 7516, completion tokens 12616, total tokens 20132, declared cost `unknown`
- cache directory digest `831505b920e444eb520d3b24c36f1dd21b15025b8d3ffe2fd226d5ae7b08bafd`, unchanged after the call

Approved bodies retained behind `UNREVIEWED:`: party `Required text key, distinct on every row.`; account `Unique numeric account identifier.`; display name `Text display name, absent on some rows.`; phone `Required text in the phone column, distinct on every row.`. Absent from both role files: entry `Int8 entry identifier; statistics show one distinct value per row.`; `national_id` `Required text in the national_id column, distinct on every row.`. The restricted role omits the display name and the phone text. `hidden_columns[0]` is `national_id` with `absent` true. Effective column `review` stays null. Dropping `column_descriptions` from `include_unreviewed` removes the party and account bodies. Stored settings still list `dataset_descriptions` and `column_descriptions`. The curated file SHA-256 stays `c7e75f0cdf6dc5c7c8634e1dd9bb34797479e035d6e31330623ecb85b8ad6a23`. Canonical JSON of that file is `b103219f47f0cbdf128491b1762bd77444dd42cde85313e358a05654468cf3c0`. The in-memory curated object is unchanged. Stored party `field_publication_seam.id` stays `description-candidate.dataset_only`. Stored `complete_command` bytes stay in place. Rebind copies model bytes, records `stored_generation` `unchanged`, producer `c23db28dc27f5cf20741dd36e8a8e88129ad1715`, stored prompt `oo.builder.description-prompt/v1`, rebound prompt `oo.builder.description-prompt/v2`, and a rebound request hash different from the stored request hash.

## Former proof still current

No new grok-4.7 call. No PostgreSQL start. `generate.rs`, `cache.rs`, `rebind.rs`, and `http.rs` blobs match `a13b1cc`. Mounted receipt hashes:

| File | SHA-256 |
| --- | --- |
| `shop.party-batch0.json` | `ef3e6cd75fcdd533d3ca042a7f8c0aa9ade85ee29d4b1213f0be5047a1fcceb8` |
| `shop.account-batch0.json` | `99495bc18617b8bb2f1e73dd8dc5cab5592a4bd3412ee73b4c600e6f1907de72` |
| `shop.entry-batch0.json` | `f71b8908087159963bb76793fd5ffefddd4d317c154815a2954032e3ddad5f6c` |
| `curated.json` | `c7e75f0cdf6dc5c7c8634e1dd9bb34797479e035d6e31330623ecb85b8ad6a23` |
| `cards.json` | `0f6890749c5fc84529d47923fa3625baf36267d01493fd2ba9356d8b95660424` |
| `catalog.json` | `89c35fd792735ed893e38123ea754406d15ff0d91968f00c9b554eec793c7ea6` |
| `profile.json` | `a4aa68072ce29144f2299e6dd32765a9977aaf595891f28012fbd0efe02ebc49` |
| `seal.json` | `4aa672d0e2c953b19312028fbe33933078e697b47389d9ded36ec71713548385` |
| `operator-review.json` | `51e0c821f0297a9bbeea3e564e9b9843321e995194e1170f5a8b369a8838d2af` |
| `settings.json` | `7e45f2b6f3c2591fd0fe5c3a3e2396621c6f195c88b53e9ab698770719c2da68` |
| `classifications.json` | `3f8c3b294461bc168ed98074f04e680e585da82d8ebc1e7bbfc14f2c48d04d66` |
| `roles.json` | `0bcfe9c1747897bf69820d9c2c1decf070b86f4b26110ec5ddc3840f2331796d` |
| `timeout.json` | `8913961628ddf82b914aa0a180b7e9ddefe6da4a94de86981aa64ab0d9d958b6` |
| `unavailable.json` | `9f2d74a9ccf36ef41bb3531f4bcf415b18a1e8cdf3580160bd65d171e5d79aff` |
| `pass2.json` | `3af5d55de0564c7cb512369f19adb27ee880351b23e5b653d27c13529b270330` |

Pass-1 meter, each `grok-4.7`, HTTP 200, socket `opened`, attempt 1, finish `stop`, status `complete`, declared cost status `unknown`, amount null, currency null, invoice false, billing certificate false:

| Dataset | candidate_sha256 | input_sha256 | request_sha256 | request_input_sha256 | tokens |
| --- | --- | --- | --- | --- | --- |
| shop.party | `1064ccef6200c2838fee04ef7bcdbbc3e240d926a3768ab5002e1d19163aeb14` | `3b79b90db87aa7bc292d387beec8731ffdee473bb1c2e9724e827b41ab695745` | `aa23e5cdd547e3dfadf8d55f0505e894978e2af029001c3e83cfa3dc1555acbf` | `98778062646b2f5e9a8a4f65a9478c343162da9d120cd5bf38821c98b0194439` | 2519 / 3869 / 6388 |
| shop.account | `fad062220149123ab2f978fa7db4444f7710143a6fd34d0c8a0650662b88edeb` | `681fe64efbb169b1473c5354db37181584000d6ce55afae0b890d3a2d29b2973` | `53d2cb58d8289d6b3bbfc0d87ec5ba1bba65fc5fe239c16a31a2cb339ec3f3ac` | `ecceaf60cdd1228066e8d58151cc044a7545f14daccaa5333af273dfb793e431` | 2473 / 5351 / 7824 |
| shop.entry | `57c79505b23f4304169e9ff72c2d9410da5f1063371f8913296ef9b442fec9a1` | `c1ebd20685b233a10ccca49a8520fa703d92a24cee11d9dd547ca155c92f404f` | `8aa7da620394b588a1bfb9f82c8b11d7871de7a60ff59ab303c3d368b00e349c` | `38881a01da268738d1ec18c9973184f506f72b6493221b1a601ac04369d6bbd1` | 2524 / 3396 / 5920 |

Cache files named by `input_sha256`, audited against those candidates: party `595392a06601ef5e6f4f2aa74dee1dd28355f80752e8caa65942ab4b1c4ea3ea`, account `2b51ca624a3cae1801c8b90b65269d5ae5fb603675aec078c2c148ecd896c5cc`, entry `db1b29dd92ace729a5ea032398bab000e8c58fff017d64f90d0661a8117e8a9f`. Two sibling cache files were present and were not the packed gold: `c90196174a52e13a82aefda7980e1834175e20a83bf10b5107d2172153648d90.json` SHA-256 `5f9a190146cccfaad4392c414ba0408eaec4af3b2be6a74daf138018a53988fd`, shop.entry candidate `cc9277395f30930dba34070235b1c2cc241173751eb8ac265c8a28a30d6c35d9`; `f8a66ef81b4c1648b79a29285fc63bc90fdcc6c1f78c2d3a984695848b28197a.json` SHA-256 `0e11b0bd719a492a90ad52a8806e409b96b4f3a9e8893758dbafe7904738d9a5`, shop.account candidate `948f3f59dd100c432b0060cf9ff4d09131c340092e73888270695cfa0752d51e`. The cache directory digest covers them and was unchanged.

`timeout.json` and `unavailable.json` refuse `pack_combined`, `review_only`, and `pack_pair` with a code ending in `/review.blocked`. `pass2.json` refuses `pack_combined`. Its `shop.party` member is `structural_draft`, refusal `evidence_identity`, cache `not_stored`, and refuses `review_only` and `pack_pair` with `/review.blocked`. Receipt file hashes after those calls match the table above.

## Independent fixture

`overlay/a369_review.rs` uses reviewer `a369-reviewer`, date `2026-10-08`, source `a369fix`, database `a369db`, project `a369review`, tables `public.shelf`, `public.bin`, and `public.ledger`, and bodies `SHELF_ID_BODY_a369`, `BIN_ID_BODY_a369`, `BADGE_BODY_a369`, `LEDGER_ENTRY_BODY_a369`, `LEDGER_MEMO_BODY_a369`, `NATIONAL_MARK_BODY_a369`. Generation goes through `seal_request`, `assemble_cards`, `scaffold_curated`, and `generate` with an in-process echo transport. The injection sentence is in the comments document and in a posted user payload, and is absent from `pg_builder::PROMPT` and from the role text. The combined pack keeps the shelf and bin bodies behind `UNREVIEWED:`, omits ledger and national-mark prose, and the restricted role omits the badge. `include_unreviewed` without `column_descriptions` omits those bodies. `pack_pair` on one table omits the other table's body. The combined protocol differs from the per-table protocol. The combined role text differs from either concatenation of the two per-table role files.

Refusals leave the curated canonical digest and the sentinel directory digest unchanged, and they create no `pg-builder` cache directory. Stale is a zero schema hash passed to `apply_description_pack_combined`, which `pack_combined` calls, and the code ends in `/review.stale`. A repeated shelf generation is `/review.double`. A second classification row for `shelf_id` is `/review.conflict`. An aggregate plus another file is `oo.pg-builder/v1/review.mixed_scope`. Fixture timeout and unavailable generations are `oo.pg-builder/v1/review.blocked` for `pack_combined`, and `/review.blocked` for `review_only` and `pack_pair`. Mixing the fixture shelf generation with the stored party generation is the same mixed-scope code.

## Failed attempt

`linux/failed-attempts/proof-1`. Container `oo-a369-proof`, id `bdf6f5e1a9184b8f884938bffb569d881762f92810621e8ec112e4b0dd3899a7`. `docker-run.exit` 101. OOMKilled false. Same CapDrop, memory, CPU, and pid limits. Pre-oracle combined tests and Clippy exited 0. `test-oracle.exit` is 101. `stored_party_and_account_share_one_core_sealed_pack` compared canonical JSON `b103219f47f0cbdf128491b1762bd77444dd42cde85313e358a05654468cf3c0` with the curated file SHA-256 `c7e75f0cdf6dc5c7c8634e1dd9bb34797479e035d6e31330623ecb85b8ad6a23`. The file pin had already passed. The stored `pack_combined` call had not run. The oracle now asserts those two digests differ, and still asserts the file pin and the unchanged in-memory canonical digest. No product edit. The proof-1 console was not overwritten.

## Limits

SP07 stays open. Public authentication stays open (`authenticated` false, `parent_auth` `required`). Semantic publication stays `not_claimed`. Publication stays `unavailable`. Release stays open. `parent_acceptance` is false. This packet does not edit the ledger and does not check E47. Numeric oid 1700 is outside this packet. The two extra cache siblings above are different candidates and were not packed.

Recipe origins are in `origin/recipes.txt`. Source hashes of the archived product are in `source-inventory.sha256`. Evidence bytes are in `MANIFEST.sha256`. Cleanup is in `CLEANUP.txt`.
