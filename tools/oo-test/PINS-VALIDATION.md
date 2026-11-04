# H-S2-pins acceptance-version consolidation

Date: 2026-11-04. TODO: S5-H-02 (pin consolidation only), S0-H-04 correction.
Rebase onto `v0.2/product-loops` reported already up to date; prior S5 commits retained.

## Pin and consumer verification

`docker buildx imagetools inspect REF`, `--raw REF`, and
`--format '{{json .Image}}' REF@PLATFORM_DIGEST` verified all eight replaced
MySQL/PostgreSQL records on linux/amd64 and linux/arm64. Docker Hub tag API
confirmed each index digest and supplied tag timestamps. Existing license,
source and platform-created fields remain present. Oracle pins are unchanged.
Raw registry evidence: `evidence/h-s2-pins-20261104/`.

PostgreSQL 17.10 is retained only for legacy consumers:
`testing/infra/compose/compose.yaml` (three services) and
`testing/infra/compose/dns-check.sh` (explicit pin lookup). Neither that record
nor legacy MySQL 8.4.10 is eligible for v0.2 resolution.

T1's separate JSON override and resolver monkeypatch are removed. T1, T2 and
T0 resolve through the shared v0.2 lock; MySQL T1 receipt validation resolves
major aliases to the exact patch before comparing. The nightly matrix uses
5.7 rather than a patch literal. Grep of executable oo-test Python leaves no
engine patch selectors outside the lock; expected patches remain in tests.
Historical evidence summaries are deliberately not rewritten.

## Full-scale fixture T0

Commands (executed serially, one database container at a time):

```sh
tools/oo-test/oo-test matrix --engine mysql --versions 5.7,8.0,8.4,9.x --suites T0
tools/oo-test/oo-test matrix --engine postgres --versions 12,13,14,15,16,17,18 --suites T0
```

Both commands exit 0. Each uses 2,000,000 events, 200 small tables and the
300-column shape. All 20 gold answers match the first version within each
engine; that first version is reference-only, not independent correctness
proof. Exact server versions were queried, not inferred from tags.

MySQL run: `h-s5-20261104T081928184364Z-bc53d888-114f-429c-897c-949d0b292355`.
PostgreSQL run: `h-s5-20261104T082101083971Z-2b26f53d-a1e9-4000-959f-e10d004e30cb`.

| Engine/version | Execution | Fixture T0 | Target SHA256SUMS sha256 |
|---|---|---|---|
| mysql 5.7.44 | emulated (linux/amd64) | pass | `bc605fed5e81b5aa967c2572f70f507de5cb2c6ae31981e85e0a5754184e9854` |
| mysql 8.0.44 | native (linux/arm64) | pass | `8b7ec83b1d1d6ec23f2259f2ac51a094bee50a959794dda38a1bc9e917ccc241` |
| mysql 8.4.7 | native (linux/arm64) | pass | `d6a300402650fc5255ebbed9587e0201c8bc867450434b92f68d05d1b87db43f` |
| mysql 9.5.0 | native (linux/arm64) | pass | `37f56cd7643eab25871ee87af802be5677e18e60638a6a06ca0ef8ec0df03ca3` |
| postgres 12.22 | native (linux/arm64) | pass | `2332f49f6cfea319eaceb021860ec45e9b0779bd5c8b293f9a940a16b6e445b4` |
| postgres 13.23 | native (linux/arm64) | pass | `b3cbe445b930f76bcdd5d4d66ef6f504d2493a45d946306a9f85dc6da4c72982` |
| postgres 14.24 | native (linux/arm64) | pass | `0d9c836b9044795d22ebc36fc4842119fb31dba1309cc4b63dd508856615e6b6` |
| postgres 15.19 | native (linux/arm64) | pass | `0f5c4c3cc14dc608e440f62ce3435eeeb1a36eea48862d4721e47e2919c53d16` |
| postgres 16.15 | native (linux/arm64) | pass | `0e2b1bc719f9cce9d615c72a606b6c66202181ada45acafba8f23bdbf2540ee3` |
| postgres 17.11 | native (linux/arm64) | pass | `054e06d63b7f26ffe2d489ce4e1752da2be97ccdcfb4c7b11a0ecd72fe8cc171` |
| postgres 18.6 | native (linux/arm64) | pass | `b82cce3a0113dc0f9233e120381fe5b7d3062ac040dcad23573de011692ea68d` |

All evidence manifests were recomputed and matched. All 11 owned containers
and volumes were individually inspected after cleanup and confirmed absent.
No unrelated resources were removed; pulled images remain cached.

## Other validation and limits

- `cargo check --workspace --locked`: pass; Cargo.lock unchanged.
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/oo-test -v`:
  44 tests pass, including exact patch selection, duplicate pins, exclusion
  of legacy versions, and the shared T1/T2 resolver.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass.
- `python3 execution/v0.2/render_todo.py --check`: pass. An initial attempt
  at nonexistent `specs/scripts/render_todo.py` exited 2 and is preserved.
- No Rust crate changed; crate formatting/tests are not applicable.
- Both platform manifests are verified, but runtime testing uses native
  arm64 except MySQL 5.7 (amd64 emulation). No second-architecture runtime
  acceptance is claimed for the dual-platform images.
- PostgreSQL 12 remains experimental. Oracle was not rerun.
- Fixture T0 is not product doctor: evidence-index rows remain `skip` for
  product admission. T1/T2/T8 were not rerun, and no capability is promoted.
- S5-H-02's remaining nightly stability exits remain coordinator work.

Resolved registry metadata sha256: `30928ef4e3680bee40b2cd4faa27a379b6c4833eb3578062e6afaa5e43f064e3`.
