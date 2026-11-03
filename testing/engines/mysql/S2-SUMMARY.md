# Lane C S2 partial evidence

## Digest primitives

`SourceEngine::digest_range` now sums both MD5 halves modulo 2^64 instead
of XOR. Equal duplicate pairs no longer cancel in no-key tables. The
`md5sum2:` algorithm tag deliberately differs from old receipts, so they
cannot authorize a no-change decision. Malformed aggregate fields fail
closed instead of becoming zero.

- Regression: `c-s2-digest-regression-20261104`, MySQL 8.4.7, original
  adapter fails the new duplicate-pair assertion with two identical all-zero
  XOR digests despite changed rows.
- Final: `python3 testing/engines/mysql/run.py --suite s2-digest --run-id
  c-s2-digest-20261104-live02` passes on 5.7.44 (amd64), 8.0.44, 8.4.7 and
  9.5.0. Lane H shared seed: 2,000 events, three small tables.
  `comparison-report.json` in each version reports equal source/adapter
  rows using lane H's comparator, with explicit quiescent consistency B.
- Earlier `live01` failures remain: the fixture database inherited latin1
  on 5.7, and the test capture helper queued split children behind later
  ranges. The fixture now declares utf8mb4; retries preserve source order.
  No gold values or equality assertions were relaxed.

This does not complete S2-C-02: the tests do not run a four-layer C5
scheduler, a target, generation publication, or a multi-source handoff.
