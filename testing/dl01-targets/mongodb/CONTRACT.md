# MongoDB preparation contract

Status: preparation probe. Not an implemented `oo.source.mongodb/v1` connector.

## Pin

| Field | Value |
|---|---|
| Image | `mongo@sha256:d0d926f94df099bff534b7ee5b5986458131a22489dfff8664509af0c1e2ca9c` |
| Server | MongoDB 8.0.32, git `f9eb55a7cc900f33a722a5b32a0fdbf54385c749`, Community |
| Probe client | mongosh 2.12.0, Node.js driver 7.5.0 |
| Topology | standalone, `bindIp` 127.0.0.1, no host port, auth enabled |
| Memory | 1610612736 bytes container limit, WiredTiger cache 0.25 GB |
| Consistency claimed | B |

A replica set is not started. `hello` has no `setName`. An explicit `$changeStream` returns code 40573, `The $changeStream stage is only supported on replica sets`.

## Corpus rules

N = 100. Presence: `present * 10 >= n * 9`. Consistency: `dominant_count * 100 >= present * 99`. Tie break is the higher count, then BSON type name ascending. Null is present. A missing key is absent. Arrays, objects, and a dominant null stay in the document. New fields under the presence bar are unclassified and `default_visible: false`.

Stable projections, when a Rust consumer exists:

| BSON | PostgreSQL |
|---|---|
| ObjectId | text, 24 hex characters |
| int64 | bigint, not float |
| int32 | int4 |
| decimal128 finite | numeric |
| decimal128 NaN, Infinity, -Infinity | row-level reject, raw document kept |
| date | timestamptz, milliseconds |
| binary subtype 04 | uuid |
| binary subtype 00 | bytea |
| double | float8, binary64 caveat |

Canonical extended JSON keeps `$numberLong`, `$numberDecimal`, `$oid`, `$date.$numberLong`, and `$binary`. Relaxed JSON is rejected as preservation: `Long("9007199254740993")` becomes `9007199254740992`.

## Proposed connector API

Not implemented. Do not treat mongosh or a missing `pymongo` import as this driver.

```json
{
  "protocol": "oo.source.mongodb/v1",
  "status": "pending",
  "capabilities": {
    "catalog": "probe_only",
    "bounded_read": "probe_only",
    "cdc": false,
    "cdc_reason": "TOPOLOGY_STANDALONE_REJECTS_CHANGE_STREAM",
    "type_conversion": false,
    "type_conversion_reason": "DRIVER_MISSING",
    "registry_api": false,
    "source_ddl": false,
    "source_write": false
  },
  "driver": {
    "kind": "P1",
    "name": "mongodb",
    "version": null,
    "digest": null,
    "status": "DRIVER_MISSING"
  },
  "probe_client": {
    "name": "mongosh",
    "version": "2.12.0",
    "role": "preparation_probe_not_product_driver"
  }
}
```

Frozen `crates/oo-connectors` at the base SHA exposes `oo.source.pgsql/v1` only. `Cargo.lock` has no `mongodb` crate.

## Pending

Rust driver, Arrow local consumer, Linux/MSRV compile, replica-set change stream and DBP6, enterprise audit, snapshot consistency above B, child-table splits, threshold recalibration, and matrix promotion.
