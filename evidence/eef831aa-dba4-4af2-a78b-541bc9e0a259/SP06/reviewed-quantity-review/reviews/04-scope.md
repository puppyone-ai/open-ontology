# Review 4 — scope and containment

Scope: owned evidence, frozen archive, builder limits, and secret markers. No builder for this note. The inventory builder runs after these notes and is not part of the verdict.

## Checks

Product crates, contracts, `Cargo.toml`, and `Cargo.lock` were not edited by this review. The compiler input was `git archive` of `e447249ab1a7f4c82df4729090b1285c9ef4a4ec`. Lock file sha256 is unchanged. Overlay registry triples were not added or removed.

Successful hostconfig: `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, `NetworkMode=oo-a259-eef831aa-net`, `OOMKilled=false`. Attempts 1 through 3 use the same limits. PostgreSQL is one private container, 1g, 1 CPU, no host port, `pg/up.exit` 0. The client mount listed `ca.crt` and the four password files. It did not list `ca.key` or `server.key`.

`linux/run/secret-scan.txt` is `clean`. The scanner in `linux/run/inside.sh` searches the evidence tree except `linux/`, where the script itself names the markers. Request JSON uses `password_file` paths under `/secrets/`. Password files stay under `/tmp/oo-a259-eef831aa-dba4-4af2-a78b-541bc9e0a259/` and are not in this tree.

The harness is an overlay member of the archived workspace. It is not a product crate and not a new component.

## Verdict

APPROVE_LIMITED. The review stayed on the frozen quantity cell and the one-table int8 boundary. Containment on the recorded runs matches the session limits. No scope expansion from this review.
