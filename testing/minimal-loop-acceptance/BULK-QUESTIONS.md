# Sealed bulk questions

The preserved 410 campaign is import `49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73`, generation 1, snapshot `00000004-00000013-1`, measured on `oo-a410-tgt` / `mirrordb` as `oo_mirror_read`. A fresh import of the same public fixture keeps these cells and records its own import hash. These prompts are the acceptance set. They are not the shop fixture questions.

Give these prompts to the model unchanged. Do not add the gold cells, the oracle SQL, or a suggested statement.

| id | prompt |
| --- | --- |
| count-batch-c | How many rows are in public.batch_c? Write the SQL yourself and run it with oo query. |
| id-overlap | How many public.batch_a ids also appear in public.batch_c? There is no foreign key between these tables. Write the SQL yourself and run it with oo query. |
| amount-when-qty-above | What is the exact sum of public.batch_c.amount for rows whose qty is greater than 2? Keep the exact numeric text. Write the SQL yourself and run it with oo query. |
| null-amount | How many public.batch_a rows have a null amount? Write the SQL yourself and run it with oo query. |
| batch-a-amount | Return the amount of the public.batch_a row whose id is 2. Keep the exact numeric text. Write the SQL yourself and run it with oo query. |
| batch-c-qty-bounds | Return id and qty for the public.batch_c rows whose id is 1 or 2, ordered by id. Write the SQL yourself and run it with oo query. |

## Gold cells

The reviewer oracle is `linux/gold/measure.sql`. It is not agent input.

| id | cells |
| --- | --- |
| count-batch-c | 90000 |
| id-overlap | 2000 |
| amount-when-qty-above | 134997.0000 |
| null-amount | 1 |
| batch-a-amount | 1234567890123456789012345678901234.1234 |
| batch-c-qty-bounds | id 1 qty 9223372036854775807; id 2 qty -9223372036854775808 |

An answer must cite the same gateway `query_id` and these cells. Do not change a cell after a model answers.
