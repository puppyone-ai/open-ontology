# E-S5g: native DM8 server readiness, not product acceptance

Plan: S5-E-03. Run: `e-s5g-20261105`. Date: 2026-11-05.

```sh
python3 testing/engines/dm8/native_preflight.py --run-id e-s5g-20261105
```

Exit 0. The owner-authorized local image
`sha256:8ddae8147e7a3a3f9bb20c0988a35857dfcaf1bbf2944a1babc5a7f513274f75`
initialized a database, reached `SYSTEM IS READY`, and answered a live
`SELECT * FROM V$VERSION` through the vendor client. Measured architecture
`aarch64`; server banner `DM Database Server 64 V8`; build
`03134284604-20260707-335949-20228`. This is native, not x86 emulation.

Admin passwords are generated per run into ignored mode-0600 files and
sent to a private initialization control file over stdin. They never enter
process argv or committed/logged output. The smoke test uses only its own
lane-E/run-labelled container and removes it in cleanup. It neither
rebuilds nor distributes the vendor image.

## Preserved diagnostics

- `dm-init.log`: stdin passwords without CONTROL do not initialize.
- `dm-init-control.log`: CONTROL initialization succeeds beneath `/tmp`,
  but `dmserver.log` shows CTL_PATH validation error -803.
- `dmserver-strace.log`: the control file exists; path validation fails
  after examining `/tmp`. This is not a proven CPU, license or client fault.
- `dm-init-v2.log`, `dmserver-v2.log`, `dm-version-v2.log`: initialization
  under `/opt/dmdbms/data` boots and queries successfully.
- `dm-native/report.json`: the independent reproducible run also passes,
  with cleanup exit 0. `dm-native/index-rows.json` explicitly skips product
  T0, T1 and T8. No server smoke test is promoted into product acceptance.

The installer includes DM's Python, JDBC and native clients. This run did
not select/admit a Rust driver, build a thick source worker, or audit a new
dependency. No dependency or Cargo.lock changed.

## Remaining card exits

Feature-gated DM8 SourceEngine, oo-load dispatch, product doctor, full
concurrent-writer Agent DB comparison and T8 remain **not implemented**.
The matrix entry remains candidate; only the obsolete license/image
blocker is replaced with the measured server-readiness and product gaps.
The whole card is partial, not complete. Separately, Db2 product T0 is
blocked by the coordinator-owned doctor allowlist; its exact patch is in
`testing/engines/db2/S5G-SUMMARY.md`.

Validation:

- `python3 -m unittest discover -s testing/engines/dm8 -v`: 8 pass.
- `cargo check --workspace --locked`: pass.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass.
- Both exploration and final smoke-test containers were removed.
