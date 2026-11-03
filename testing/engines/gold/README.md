# Source-computed gold

`questions.json` contains Q01-Q20. The matching `<engine>.sql` files are the
query contract; every query has deterministic ORDER BY when returning groups.
The common SQL subset currently permits identical dialect files.

`oo-test matrix` executes each query using a read-only source account and
writes `gold-answers.json` inside that run's ignored evidence. No fabricated
or locally precomputed answer is accepted as a source result. Pass another
engine's answer file as `--baseline` to require cross-engine equality.
The bulk event questions depend on `--scale`; compare only identical seeds.

Q09/Q10 intentionally define empty and NULL as missing through NULLIF so
Oracle's native empty-string behavior cannot silently alter the question.
Raw type fixture semantics remain distinct. The gold text transport is only
for these controlled numeric and ASCII-label queries; it is not the general
value-comparison API (see `tools/oo-test/comparison.py`).
