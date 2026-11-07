## Card A-S3S4 — lane A: SSH, owner self-review, oo build, ontology export, remote deploy

TODO: S3-A-01, S4-A-01, S4-A-02, S4-A-03, S4-A-04. Lane label: `a`. Spec: REQ-ONB-04/06, AT-ONB-01/03.
Requires A-S1S2, engine adapters, lane G budgeter and lane B tokens integrated (rebase first).
Deliver:
1. S3-A-01 SSH jump host with `russh` direct-tcpip per read connection; host key fingerprint confirmed on
   first use and pinned as host_key_sha256, mismatch fails; keepalive; reconnect resumes at chunk
   boundaries. Test with an sshd container (labelled oo.lane=a) in front of a DB container, including a
   forced disconnect (T3) and a fingerprint mismatch. Dependency audit record for russh.
2. S4-A-01 owner self-review in the TUI: summary of entities, relations, metrics, sensitive-column
   classification; per-category accept/reject; writes `review_decision` with `reviewer=owner, mode=self`;
   publishing without a review decision is impossible; automatic acceptance is never recorded as human.
3. S4-A-02 `oo build [--resume]`: full read → Self-build (lane G budgeter via model-egress) → self-review →
   publish → Gateway → issue first token (lane B enroll) → write the agent prompt (`oo share-prompt`).
   Every stage resumable after interruption (test: kill during each stage, resume completes, no duplicates).
4. S4-A-03 `oo ontology open | export --format html|mermaid|yaml` from `relationships/graph.json`: one
   self-contained offline HTML file (vendored JS inside the file, no network), usable with ≥1000 entities.
5. S4-A-04 `oo up --target ssh`: install the lite compose stack and a systemd timer on a remote Linux host
   over SSH, writing only under its own directory, no silent sudo. Test target: local containers running
   sshd + Docker (dind or socket-less rootless) for linux/amd64 and linux/arm64.
Write scope: crates/oo-cli/src/admin/**, crates/oo-stack/**, deploy/lite/**, worker SSH transport files
(name them; coordinate via report if they live in tools/job-worker), supply-chain records, lock (report).
