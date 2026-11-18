# L7 evidence (30 section L7 and 7; B-S11-01 .. 04, B-S12-01, B-S12-02)

`tools/oo-test/super_ontology_l7.py` run `a7-l7rem` (after the L7 remainder): 41 of 41 checks pass (`l7-e2e-result.json`, `l7-e2e-log.txt`). It runs the
cargo suites below against a labelled PostgreSQL (`postgres@sha256:839fbb5a...`, `track_commit_timestamp=on`), then starts two real
`oo-docs-engine` processes under two real `oo serve` (a local engine A and a remote engine B, each with its own database, state
directory and Drive over the same files) and drives them with the real `oo` CLI.

## What could not be done: no real bucket

No OSS, COS or R2 account exists and none may be created. Nothing here claims a real cloud was reached. The "not confirmed" cells
of 10 4.3.7 are still not confirmed by a real store; what is proved is that the code does the right thing when a store answers the
way the clouds document it, against a look-alike (an in-process server, and with an ignored test a header injecting proxy in front of
MinIO or RustFS). `crates/oo-connectors/tests/cloud_live.rs` is the real bucket test, ready for the owner:

```
OO_CLOUD_BACKEND=oss|cos|r2|s3  OO_CLOUD_BUCKET=<bucket>  OO_CLOUD_REGION=<region>  [OO_CLOUD_ACCOUNT_ID=<r2 account>]
OO_CLOUD_SECRET_REF=file:/abs/path/credential.json      # {"access_key_id": "...", "secret_access_key": "..."}; never the key itself
OO_CLOUD_EVIDENCE=/tmp/cloud-live-<backend>.json         # what the store answered, no secrets
cargo test -p oo-connectors --features objectstore-oss,objectstore-cos --test cloud_live -- --ignored --test-threads=1
```

It writes only under `oo-cloud-live-<pid>/` and removes it. (`objectstore-oss` and `objectstore-cos` are aliases of `objectstore-s3`,
patch `B-S11-01-features.patch`.) RustFS and MinIO images could not be pulled (no route to the registry), so the existing
`s3_rustfs` suite was not rerun in this round; the `cloud_mimic_minio` ignored test was not run either.

## Results

| Card | What | Result |
|---|---|---|
| B-S11-01 | oss, cos, r2 as `objectstore-s3` Drives; compat table | 188 connector tests pass (compat 9, cloud_mimic 7, s3_mock 6, the rest of the lib). OSS endpoint `https://s3.oss-<region>.aliyuncs.com`, COS `https://cos.<region>.myqcloud.com`, R2 `https://<account>.r2.cloudflarestorage.com` with region `auto`; OSS and COS virtual-hosted only, R2 path style; a region or account id that is not a plain label is refused. ECMA CRC64 headers (`x-oss-hash-crc64ecma`, `x-cos-hash-crc64ecma`) verified, a corrupt transfer fetched again, three in a row end in `DOCS_SOURCE_CHECKSUM_MISMATCH`; an R2 like store reports no CRC64, no versions, a multipart ETag as a change signal only; a `Date` ten minutes off is reported as clock skew. `COMPAT_TABLE` says per row how it was established (`mimic`, real bucket pending) |
| B-S11-02 | event notifications and dedupe | 11 unit and property tests: 300 histories of puts and deletes, every event delivered late, out of order and partly twice, the view built from the `stat` answers equals the store; sequencer order (zero padded text compare); S3, R2, OSS formats; a local spool source. Over HTTP (`events_live`, 1 test) and on the real engines: a new file is published 5.2 s after the event with the poll set to 3600 s; a repeat is counted as a duplicate, an older event as stale, a delete event is only reported |
| B-S11-03 (remainder) | parallel listing per first level prefix | A Drive on an object store lists its first level prefixes four at a time (`config.listing.parallel` overrides, 1 lists whole; fs defaults to 1). Each prefix has its own row and generation counter in `_oo_docs.drive_sync_prefix` (migration 0149): the counter moves only when that prefix listed completely. A prefix that fails to list is not swept and does not hold the others: the complete prefixes are swept, the run is reported failed and the Drive generation stays uncommitted, so the retry lists again. A prefix whose directory has vanished lists as empty and its files are swept. `""` is the files directly under the root. Evidence: fs scopes partition the listing (2 unit tests), bounded parallelism (2), `prefix_sync` on PG (4: parallel with own counters, a failed prefix, whole listing at parallel 1, an S3 Drive on a real MinIO by default), S3 scopes on a real MinIO (`s3_rustfs` `scopes_partition_the_listing`, page sizes 1, 2, 100). Existing `ingest_fs` (15) and `conformance_sync` still pass; the formal trace conformance of a partial sweep is not extended (D81) |
| B-S11-02 (remainder) | pull adapters, COS format | `EventQueue` (receive, ack) and `pull()` give the same hints as the local spool. SQS (JSON protocol, SigV4), Cloudflare Queues (`messages/pull` and `ack`, bearer) and Alibaba MNS (batch receive and delete, HMAC-SHA1) were written against the public API descriptions and tested against fakes and one local socket only (11 tests): SigV4 matches the AWS signing key and get-vanilla vectors, SHA-1 and HMAC the RFC vectors; a message is acknowledged only after its hints were handled, an unreadable body is left for the queue, an SNS envelope is unwrapped, a redelivered message is absorbed by the deduper, SQS deletes in batches of ten. **None has been tried against the real service; the MNS signature in particular is unconfirmed.** COS: the Tencent documented notification format parsed from three synthetic fixtures (key `/appid/bucket/key`, time in Unix seconds, delete marker). Not confirmed against a real bucket |
| B-S11-03 | Drive policy | `policy_live` (2) and 7 unit tests: with `{"deny": ["Payroll"]}` on both engines the denied file is never listed, a mount of the whole Drive carries nothing of it; a targeted sync and an event for a denied path do nothing; a file that becomes denied is swept; the visible graph drops a denied path before any sweep |
| B-S11-04 | projection into the Agent DB, links | `projection_live` (2) and 3 unit tests: idempotent (second run writes nothing, same digest), rebuildable (drop the schema, same digest), a person's rejection survives rebuilds, the policy takes a denied contract out. Demonstration `projection-demo.json`: contract document of 澄海物流 is linked to customer row 17, the other document to row 19, 北辰云 (row 18) has no document and no link. The contract there is Markdown, see the test header |
| B-S12-01 | cloud tokens, audit | `audit` (2): the `oodc_` token is stored as a hash, refused on the loopback entry, the control token is refused on the cloud entry, a Drive outside the token's set and any other route are 403, revoking the token ends it; the audit answers who shared what to which agent and when (`audit-who-shared-what.json`: agent-7 through the cloud entry of tenant acme, scope, role, grantor, revoked at, reads of refs), holds no credential, cannot be updated, deleted or truncated (not even by the owner), and the hash chain verifies (a forged row is found) |
| B-S12-02 | `oo context mount --remote` | two stand-in tests and, on the real engines: the mount through engine B's cloud entry (with `--pack-digest` of the local mount) equals the local mount of the same engine byte for byte except `SHARE.json`; the token is in no output and no mounted file; a mount with another digest leaves nothing mounted and revokes its share |

## Notes for the coordinator

* Packs of two engines over identical files are not equal (a different Drive id and ontology history change the pack digest, and the
  AIGC label of every file embeds the digest). Equality is a property of one engine: its local pack and the same pack fetched
  through its cloud entry. `l7-e2e-result.json` lists the files that differ between engine A and B (informational).
* Patches in the notes directory: `B-S12-01-migration-docs-cloud-audit.patch` (0148 and its registration, needed by `audit` and the
  L7 script), `B-S11-04-migration-docs-projection.patch` (0147, a copy of `projection.sql`; `project` also creates it on first use),
  `B-S11-01-features.patch` (feature aliases), `B-L7-contract-docs-api-1.4.patch` (routes, schemas, changelog).
