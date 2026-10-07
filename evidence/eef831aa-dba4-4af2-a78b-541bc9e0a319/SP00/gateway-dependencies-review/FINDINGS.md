# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a319`. Evidence is `linux/preoverlay-2/` unless a finding names the first attempt.

## F1. httpdate is enabled by hyper's `server` feature

Axum 0.8.9 feature `http1` lists `hyper?/http1` and `hyper-util?/http1`. It does not name httpdate. Axum's `hyper-util` dependency enables `tokio`, `server`, and `service`. hyper-util 0.1.20 writes `server = ["hyper/server"]` on one line. hyper 1.11.1 feature `http1` lists atomic-waker, futures-channel, futures-core, httparse, and itoa. Feature `server` lists `dep:httpdate`, pin-project-lite, and smallvec. The httpdate dependency is optional `version = "1.0"`.

`axum-edges.txt` records `httpdate_edge=hyper-util-server-enables-hyper-server`. `hyper-httpdate.txt` records `hyper_httpdate=server`. The author sentence that hyper's `http1` feature pulls httpdate does not match these crate bytes. The pin of httpdate 1.0.3 still stands, because the enabled path is real. This is not a reason to withhold the baseline.

## F2. Lock identity ignores dependency-array edits

`lockdiff` identity is name, version, and checksum. The passing subset report is `added=5`, `changed=0`, `denied=0`. `edges.txt` also prints `edge_diff_count=13` and `edges=pass`. Those diffs are dependency lists of packages that already share a checksum.

hyper 1.11.1 is the load-bearing one. Root lock dependencies include `h2` and `want` and omit `httpdate`. Probe lock dependencies include `httpdate` and omit `h2` and `want`. The checksum in both locks is `27b501faa50e7a26c3d3560ca625132f4078a17771f4810baf70475ae48cbe43`, which matches the official crate bytes.

The probe is a smaller workspace. Its feature set is http1 plus the pinned tower and tower-http features. Copying its dependency arrays onto the root lock would drop edges other crates still need. A consumer lock may add httpdate to hyper, and may add edges required by the pinned features, while keeping existing edges. It must not change a version or checksum, and it must not blanket-update. `APPROVE_LIMITED` does not approve that edit.

Other recorded diffs include tower gaining `tokio-util` and `tracing` under the pinned features, and tower-http's feature list differing from the root package. Same rule: add required edges, keep edges the rest of the workspace uses.

## F3. Two newer stable crates do not fit Axum 0.8.9

Live index, recorded by `index-newest` and not used as a bump:

- matchit `max_stable_version=0.9.2`, published 2026-04-08T04:23:31.542402Z. Axum 0.8.9 requires `matchit` `version = "=0.8.4"`.
- tower-http `max_stable_version=0.7.1`, published 2026-08-31T14:49:25.697237Z. Axum 0.8.9 requires optional tower-http `0.6.8`. The workspace pin is `=0.6.11`.

Axum's own max stable version is 0.8.9. No age waiver was applied.

## F4. The first container stopped on an oracle bug

Container `d713a1e5f7d388316fd850f13fc8abf9892420250213bfe9c81de3f9538128ad` exited 1, OOMKilled false. Crate verification, the five-triple subset, and `subset_report` had already passed. `crate_verify edges` then reported `probe adds 6 packages, pins 5` because it counted path package `oo-gateway-http-baseline-probe` 0.0.0 as a registry add. Console and phase files remain in `linux/preoverlay/`. Tests and Clippy did not run there.

The oracle now requires that local package and only that local package, and still requires exactly the five registry triples. `linux/preoverlay-2/edges.txt` records `added=5` and `edges=pass`. The miss is a review-oracle defect. It is not a baseline failure.

## F5. The author inventory still omits the host tail

Author `FINDINGS.md` F7 stands. The author hashed `/evidence` inside the proof container before `launch.sh` wrote `hostconfig.txt`, `tmpfs.txt`, `mounts.txt`, `docker-run.exit`, and `container-removed.txt`, and before `inside_ok` was appended. This review does not treat that `SHA256SUMS` as a complete listing of the author host tail.

This review's inventory runs after `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, `CLEANUP.txt`, and the proof host files exist. The hasher skips `SHA256SUMS` only.

## F6. No member consumes the pins

`members.txt` records 19 manifests and `members=pass`. No member manifest names axum or tower-http, and none takes tower from the workspace. Root `[workspace.dependencies]` contains only the three pin lines. Root `Cargo.lock` is byte-identical to the fixed archive because those pins are unused. The absence of a full HTTP consumer is not a baseline failure.

`execution/decisions/gateway-http-dependency-baseline.md` still says `Status: UNAPPROVED`. This review does not change that file. The verdict lives in `REPORT.md`.
