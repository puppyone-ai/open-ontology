# Agent QA question set (lane V)

`questions.jsonl`: 45 questions with deterministic gold, written before any agent run.
Split: 15 `dev` (prompt and CLI ergonomics may be tuned against them), 30 `held` (never edited after results).
Categories: file (15), db (15), cross (10), unanswerable (5).

Gold comes from `../golden/labels` and `../golden/docs` for files, and from the `shop` Postgres fixture for
the database (`shop_fixture_1_schema_seed.sql`, then `shop_fixture_2_skewed_orders.sql`; the second seeds
`setseed(0.4217)` before drawing the skewed orders). 4 tables, 3 foreign keys.

Gold types: `entities` (every `must` regex matches the ANSWER line, no `forbid` regex does), `number`
(value within `tol`, or any `alts`), `all` (every part), `decline` (the agent must say the data does not contain it).
`src` lists acceptable source refs (corpus path relative to `docs/`, or `public.<table>`).
