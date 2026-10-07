# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a337`. Read `CONTRACT.md`, `REPORT.md`, and `FINDINGS.md`.

Verdict: **APPROVE_LIMITED** for the frozen child at `51d08e59ddb3c92fc0b9f2958966b56e19032b7f`. The passing run is `linux/preoverlay-3/`, container `205a4c59a081`, `inside_ok=2026-10-07T14:46:09Z`, `docker-run.exit=0`. Private PostgreSQL answered at `2026-10-07T14:45:34Z` before that HTTP test. Earlier exits remain in `linux/preoverlay/` (fetch `-p`, exit 1), `linux/preoverlay-2/` (private-test Clippy, exit 101), and `linux/preoverlay-3/docker-run-defer-1.exit` (host cargo occupied the slot, exit 3).

The root lock adds axum 0.8.9, axum-core 0.5.6, httpdate 1.0.3, matchit 0.8.4, and mime 0.3.17. `removed=0`, `changed=0`, `denied=0`. Hyper 1.11.1 keeps `h2` and `want` and adds `httpdate`. The embedded store's 13 digests match. The only `clippy::all` allow is the private module glue. Arch-check passed.

The fresh HTTP proof issued a client token, selected that role's pack, rejected the other role and the other policy column, rejected forged caller headers, rejected a session token, and showed a committed revoke that stayed cached for the original 5 seconds and failed at 5.2 seconds. A closed database failed after that same deadline, failed again in a new process, and succeeded after the endpoint was restored. Readiness stayed `database=not_probed`. `POST /v1/sql` returned 501 and the HTTP sources do not call `execute_trusted_select`. Body 32 was 501 and body 33 was 413. A stall returned 408. One global permit held the next request for 604 ms, and that request then returned 200.

The operator pack is loaded from JSON. File digests are verified. `pack_digest` is a declared 64-hex field. A substituted digest was returned. This child does not authenticate that field as an `oo-builder` compiler digest.

Public mTLS, OAuth, an IdP, durable audit, and a readiness database probe remain parent gaps. The decision file header remains `UNAPPROVED`. The E45 plan lines remain unchecked. This handoff does not accept the parent and does not release.

`CLEANUP.txt` records removal of the a337 database container, the three private volumes, the private network, and the host secret directory. No prune. `oo-a327-pg` and `oo-a334-pg` were not removed.
