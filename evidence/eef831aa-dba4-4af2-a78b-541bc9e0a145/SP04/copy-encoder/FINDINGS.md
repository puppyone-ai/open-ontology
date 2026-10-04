# Findings — a145 COPY encoder

The committed codec is binary COPY only. `CopyEncoder::pull` yields the 19-byte header, bounded tuple chunks, the `0xffff` trailer, and then `Done`. System position is approved-list index 0, caller-supplied `ExactDecimal`, encoded as `numeric`. User columns follow that list. That order is the COPY order. It is not `attnum` order and it is not `TypedBatch` field order. A list with no user column is `zero_user_columns`. A list that does not start with the system column is `system_position_order`.

NULL on a nullable column is the COPY length `-1`. `UnavailablePolicy::Refuse` is the only policy. An unavailable or masked cell is `unavailable` and is not encoded as SQL NULL. Unmapped rejects stay on the batch. The encoder does not persist quality rows.

Budgets are checked before the chunk buffer is allocated. A payload over `max_value_bytes` is `value_budget`. A tuple over `max_row_bytes` is `row_budget`. A tuple that fits the row budget and does not fit an empty chunk is `chunk_budget`. A later row that does not fit the remaining chunk ends that pull. A measure error poisons the encoder and the next pull returns the same error. There is no background queue.

## What PostgreSQL decoded

The live fixture is schema `oo_copy_a145`, table `sample`, COPY list `(pos, note, flag, n, raw, amount, day, frac)`. Source attnums are flag 1, n 2, note 3, raw 4, amount 5, day 6, frac 7. The server decoded two rows inside one transaction and the test rolled that transaction back.

`numeric(8,2)` `-123456.78` came back as text `-123456.78`. `0.00` came back as `0.00`. Date days since 2000-01-01 came back as `2000-01-01` and `1970-01-01`. `float8send` of the stored Arrow bits returned `8000000000000000` for negative zero and `3ff4000000000000` for 1.25. Bytea with a zero byte, `0xff`, a quote, a tab, and a newline returned hex `00ff22090a`. The second row's note and bytea were SQL NULL, distinct from empty text. `int8` `i64::MIN` survived. Boolean binary send is one byte `0` or `1`. `boolean::text` on this server is `true` and `false`, which is what focused-5 observed and what focused-6 expects.

Local tests, not the live table, also cover time `01:02:03.000004` as microseconds `3723000004`, timestamp midnight as eight zero bytes and plus one second as `1000000`, exact `f32` 1.25, refusal of `0.1` as `float_precision`, refusal of `f32` NaN as `float4_nan`, and refusal of `numeric(10,-2)` as `numeric_scale` detail `negative`.

## Limits of this codec

The type table in `contracts/pipeline/copy-encoder-v1.md` is the coverage. `jsonb`, `interval`, arrays, enums, and any other oid are refused. Negative numeric scale is refused and is not rewritten to scale 0. `float4` that is NaN or that does not round-trip through `f32` is refused. Text containing a NUL is `text_nul`. Character limits refuse a longer Unicode scalar count. `bpchar` and `varchar` are stored bytes, not padded and not truncated. `json` is the stored text.

A domain encodes as its base oid when that base is in the table. Domain checks are not applied. Canonical `FloatClass::Zero` drops the sign of zero; the encoder sends the Arrow bits and refuses a class that disagrees with those bits.

The fixture write uses a test-only wire client on the current `std` and `oo-pipeline` dependencies. It is administration of a private database. It does not open a production COPY sink, take an advisory lock, write a receipt, or publish a read.

## Preserved failures

`linux/focused` through `linux/focused-5` are the failed attempts named in `REPORT.md`. Their exit files were not rewritten after the green run. Smoke exits were not rewritten. The first local `git commit` invocation put `-c` on `commit` and Git rejected it (`options '-m' and '-c' cannot be used together`). That attempt created no commit. The recorded commit is `d80472d8a99afd66d8d0e71420f7be2bb62e65c8`.
