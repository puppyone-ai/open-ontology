# Synthetic typed source file

This sample is not evidence that a product command runs. No current binary loads it. `oo validate`, `oo plan`, and `oo apply` are specified later. They are not in this tree.

The fields below follow [docs/12 §3](../../docs/12-configuration-reference.md). That chapter says the implementation schema is whatever `oo-core` exports, and the chapter is still Proposed. Where `oo-core` already constrains a token, the sample uses that shape: a connector name, a `secret://` reference, a timezone of `Etc/UTC`, and table names of letters, digits, and underscores. The wrapper key for a multi-source file is not settled here. Do not treat this page as `examples/config/` or as a JSON Schema.

The host is `db.example.invalid`. There is no credential value, no customer name, and no internal domain. `allow_write_capable_credential` stays false. `sample_values` stays `none`, so the illustration does not send cell values to a model. Sync is `full` because CDC is not a current capability.

```yaml
connector: postgres
connection:
  host: db.example.invalid
  port: 5432
  database: synth
  tls: require
read_from: replica
credential: secret://synth_reader
timezone: Etc/UTC
tables:
  include:
    - shipment
  exclude: []
sync: full
snapshot: oo_chunked
consistency: transactional
allow_write_capable_credential: false
sample_values: none
```

A later product command would have to accept or reject this text against the exported schema. This file does not do that, and a sample that sits in git is not a passing acceptance case.
