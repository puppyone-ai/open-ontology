# openGauss 5/6 compatible-auth adapter

`oo_connectors::postgres::opengauss::OpenGaussEngine` implements a separate
`EngineFamily::Opengauss`; it does not inherit PostgreSQL capabilities.
`version_rules()` describes the admitted 5.x/6.x behavior. Status is
**experimental**, not support-matrix `verified`.

## Authentication and DBA setup

The existing SQLx PostgreSQL driver is unchanged. Product connections require
caller-supplied CA verification (`verify-full`) and a compatible reader
verifier. The fixture uses a generated MD5 verifier, never `trust` for network
reader connections. SHA256-only (authentication code 10, overlapping PG SASL)
and SM3-only (13) are not implemented.

On an incompatible authentication challenge, `probe()` returns `NeedsSetup`
with the following DBA actions. The server version remains **unmeasured**:
major/minor/patch zero are sentinels, not an observed version.

```sql
ALTER SYSTEM SET password_encryption_type = 1;
SELECT pg_reload_conf();
ALTER ROLE "reader" PASSWORD '<new-reader-password>';
```

Put a suitably restricted rule before conflicting entries in `pg_hba.conf`,
then reload:

```text
hostssl all "reader" <reader-CIDR> md5
```

Use the actual role/CIDR and reset the password through a secure DBA client.
Changing the server parameter alone does not convert an existing verifier.
The fixture creates its SHA256-only account while storage mode is 2, restores
mode 1, and proves that account still needs setup. A successful MD5 reader is
not rejected merely because the server's default storage policy differs.
Authentication-protocol errors can also mean a malformed server challenge;
the setup report is not proof of a successfully identified server.

MD5 is a compatibility limitation, not a recommendation to weaken an existing
security policy. If policy forbids it, this adapter remains unavailable.
The old driver-boundary tests still demonstrate refusal; they are not live
engine passes. Historical D-S4 failures remain in `D-S4-BLOCKERS.md` and the
unchanged `evidence-summary.json`.

## Snapshot and reconciliation

- One engine instance holds one read-only repeatable-read transaction for one
  cycle. Drop it at cycle end. `export_snapshot()` supports independent
  same-snapshot comparison; no physical checkpoint survives a cycle.
- Key discovery, source-side key comparisons, NULL framing, binary type
  decoding, Arrow construction, LSN parsing and C5 scheduling reuse PG code
  where compatible. The fork's catalogs, RLS policy checks, authentication,
  xlog function names and version admission remain separate.
- Primary/unique keys use source-compared keyset ranges, including open ends.
  No-key tables use one bounded read. A grown/oversized chunk fails rather than
  truncating; the caller must replan in a fresh cycle.
- Only root PG lossless binary mappings are accepted. Unknown/fork-specific
  types, partial reads, decode rejects, stale plans, RLS policies and unsupported
  relation kinds fail closed. This is not a full openGauss type/catalog adapter.
- `digest_range()` uses the available core MD5 function, count and two
  order-independent 64-bit numeric sums, with explicit NULL/length framing.
  Its algorithm tag differs from PG's SHA256 tag. This is a non-adversarial
  reconciliation hint, **not cryptographic integrity protection**.
- `inspect_c5()` shares PG's positive hints, recent-first scheduling, complete
  range inspection and periodic full-read policy. Persist source digests only
  after successful target application. Unchanged cycles request no row payload
  or target rewrite. C5 is not CDC.
- Logical decoding is graded from actual read-only observations of `wal_level`,
  slot capacity and decoding functions. It remains `Unsupported`: this
  connector has no openGauss decoder/`ChangeSource`, and does not create slots,
  publications or other source objects.

## Reproduce the live fixture

```sh
python3 testing/engines/opengauss/run.py --run-id og-5 --version 5.0.3 --uniform-vm
python3 testing/engines/opengauss/run.py --run-id og-6 --version 6.0.0 --uniform-vm
cargo test -p oo-connectors --locked --test opengauss_auth_boundary
```

`compatible-images.json` pins the original enmotech EE ARM64 images. The runner
uses `--privileged`, `oo.lane=d`, loopback ephemeral ports, generated 24-character
complex passwords, ignored 0600 files and a separate fixture-admin connection.
It removes only its own container and anonymous volumes, including on failure.
Do not run more than two database fixtures concurrently.

### Docker uniform-memory VM workaround

**Omit `--uniform-vm` on a normal NUMA-aware Linux host.** It is a fixture-only
workaround, not product or deployment code, and refuses an existing NUMA
topology. Tests using it are not stock-kernel or production-environment passes.

Privileged startup alone fails on this Docker VM: MOT cannot discover NUMA
nodes. A container-local single-node sysfs view resolves CPU mapping, but MOT's
recovery allocator then receives `ENOSYS` from `mbind(MPOL_INTERLEAVE)`.
`uniform-vm.sh` makes mount propagation private before installing the synthetic
view; it never mounts into the host namespace. `uniform-vm.c`, compiled using a
pinned ARM64 builder, forwards libc syscalls unchanged except for that missing
interleave hint with flags zero. Interleaving one uniform-memory node needs no
placement operation. No database binary, SQL behavior, authentication code or
assertion is patched. The shared library exists only under ignored evidence.

Prior EE normal/privileged, ARM64/amd64 and Lite explorations are retained under
`evidence/d-s4b-20261104/`. Lite 5.0.3 was a diagnostic success, not an EE result.
The final EE evidence distinguishes this VM workaround explicitly.

## Evidence boundary

The live tests exercise T0 version/privilege/auth/decoder grading, a 2,000,000-row
table, eight snapshot tables including a 300-column table, integer/string/
composite/unique/no-key/empty shapes, concurrent insert/update/delete/PK update,
and three C5 cycles including a no-change cycle. An additional test checks exact
decimal, bytea, boolean, timestamp, JSON, NULL, budgets and forced full reads.
The independent oracle hashes source text with SHA256 and compares it with
decoded Arrow; it does not reuse the product's MD5 digest as gold.

Target application is an in-memory range-replacement oracle, **not** Agent DB
publication or Gateway/business acceptance. Case-insensitive collations,
partitioned parents, domains/enums, the full neutral multi-hundred-table model,
T3-T8, remote Linux without the VM workaround and C1 are not certified here.
See `s4b-evidence-summary.json` for exact commands, outcomes and evidence hashes.
