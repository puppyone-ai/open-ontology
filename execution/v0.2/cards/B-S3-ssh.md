## Card B-S3 — lane B (reassigned from A): SSH jump host

TODO: S3-A-01 (reassigned: lane A is on the S2 critical path). Rebase on v0.2/product-loops first. Read docs/product/04
and the stack.yaml schema (S0-M-04). Write scope: a new transport module for SSH tunnels (place it in
`crates/oo-connectors/src/transport/ssh.rs` or a new crate — choose the smaller), its wiring in stack.yaml parsing and
the engines' connect path behind one function, `oo doctor` output for the tunnel. Cargo.lock may change for `russh`
(run cargo deny + cargo audit; report). Lane A is editing `oo init` TUI files concurrently: do not edit the TUI; put the
TUI step text you need in your report.
Deliver (keep going; commit as you go):
1. russh direct-tcpip forwarding to the DB host; host key prompt on first use (non-TTY: require a pinned fingerprint in
   stack.yaml), fingerprint pinning, mismatch refusal; key and password auth via secret_ref only; keepalive and
   reconnect with backoff; clean shutdown.
2. Live test: an OpenSSH container (labelled, yours) in front of a PostgreSQL 17.11 and a MySQL 8.4.7 container on an
   internal network unreachable from the host; T1 through the tunnel; disconnect mid-chunk and verify resume; fingerprint
   mismatch refused.
