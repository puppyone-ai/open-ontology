`spool.ndjson` is the captured MariaDB GTID `0-1-7` envelope. It has no `source_epoch`.

`positive-maria.json` epoch 1 and `positive-mysql.json` epoch 1 are caller samples. `observed_in_capture` is false. They are not a source registration and not a capture epoch.

The MySQL uuid in `positive-mysql.json` is a grammar sample, not a captured server.

`source-money-gold.json` keeps unit `unspecified` and pii `unknown`. The generator gives `cost_amount` and `k_note.money` no currency and no privacy class. Those columns are not rewritten to `none` and are not an approval to publish. Unknown columns stay out of the visible classification set.
