# DL01-W1-24 fixed review

Reviewer session `99e08beb-dd6a-4de4-bcb5-801bfb2a0eed`.

Decision: `approve` the frozen child at `29597079f7feacc6e3934b5c803686fc80567a93`. Seal `b7d16ceba42543eab0163ac42679e8a8d412d8b0`. Approved scope is `source-support` only. Parent HTTP, the `oo` command, host allowlist, secret resolution, DBX-W01, DL01, a matrix grade, and a 14-night product nightly stay unsigned.

The first review `4dee46fd4b431500b8d7ce6949325a3dd6921f09` was `request_changes` on `e4e400235d5b6891df099308854501d6684965db`. That stdout is preserved: `linux/review/probes/down-apr-may.stdout` is `DOWNGRADE_HELD` for 2026-04-30 and 2026-05-01, and `down-feb-mar.stdout` is the same hold for 2026-02-28 and 2026-03-01. `day_number` at that commit was `year * 372 + month * 31 + day`.

On `29597079` the same two event files return `DOWNGRADED` / `two_consecutive_failures`, `applied` true, `to` `experimental`, `existing_source_stopped` false, `matrix_mutated` false. Independent CLI cases also downgrade 2011-04-30/2011-05-01, 2019-02-28/2019-03-01, 2024-02-29/2024-03-01, 1900-02-28/1900-03-01, and 1999-12-31/2000-01-01. 2024-02-28/2024-03-01 and 1999-12-31/2000-01-02 stay held. `1900-02-29` is `malformed` / `day`. Calendar fail count 0. Public `verified_count` is 0. Matrix sha256 `c4fc6698cfb08d74b08fcadbe1f5a6acb2ba41a769905b06328b28f3ab8944b4` is unchanged.

`cargo test --locked --offline --all-targets` exit 0: 5 lib tests and 10 journey tests. `rustup component add clippy --toolchain 1.94.0` exit 0. `cargo clippy --locked --offline --all-targets` exit 0, version `clippy 0.1.94` with `rustc 1.94.0`. Stderr has warn-level lints, including `items_after_test_module` at `downgrade.rs:214`. The author repair Clippy exit 1 was `component_absent` and is not treated as a pass.

Packet: `evidence/99e08beb-dd6a-4de4-bcb5-801bfb2a0eed/DL01/matrix-fixed-review`. Seal packet digests 482 of 482. Binary sha256 `3256c2030f0f4772fa4c753173c3ebd0d2983ccc5d35c79f282cbc504691c3d3`. Prefix `oo-dl01w1-24`. No database. The re-review container and volumes are removed. The pinned image and the author `oo-dl01w1-01` volumes remain.
