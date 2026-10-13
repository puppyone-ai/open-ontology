# Logistics product loop

Rust generator and acceptance card for the public logistics fixture.

```text
hubs              2000
shipments         8000
tracking_events   90000
```

`generate` writes `schema.sql` and COPY files. `gold` writes `questions.json` (natural language, no answers) and `oracle.json` (SQL and cells). Do not mount the oracle into the model, the pack, or Pi.

The six questions are a row count, a real hub-id cost sum, a delayed-air count, a NULL cost count, one exact `numeric(38,4)` value, and the int8 capacity bounds.

`card` judges a live product observation. It does not call the model and it does not execute SQL.
