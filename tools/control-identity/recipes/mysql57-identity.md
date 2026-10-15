# Isolated MySQL 5.7 management identity fixture

This fixture runs the normal `control-identity install`, one-time bootstrap and
OIDC PKCE/RS256 flow. It is explicitly synthetic management identity, with no
customer-owner certification or data grants. Existing `mysql` project bindings
and the identity service at port 18441 are untouched.

## Retained deployment (2026-10-16)

- Service: `oo-mysql57-identity-20261016`, HTTPS `https://127.0.0.1:18447`.
- Issuer: service loopback HTTPS 18448; not published to the host.
- PostgreSQL: `oo-mysql57-identity-pg-20261016`, database `oo_mysql57_identity`.
- Owned network: `oo-mysql57-identity-net-20261016`, subnet `10.157.57.0/24`.
- Owned PG volume: `oo-mysql57-identity-pgdata-20261016`.
- Private host root: `/Users/redux/.cache/oo-mysql57-identity/private` (0700).
- Identity CA: `ca.pem`; opaque sessions: `fixture-{admin,reviewer,owner}.cookie`
  under that root (0600). Never copy their values into argv, logs or Git.
- Project `mysql57` has actual authenticated `fixture-admin/admin`,
  `fixture-reviewer/reviewer`, and `fixture-owner/business_owner`; steward and
  auditor fixtures are also retained. All five bindings originate from install.
- Runtime uses cached `rust:1.94.0`, `cargo test --locked --offline --test live
  native_retained_product_identity -- --nocapture`, working directory `/workspace`.
  Immutable identity-crate snapshot is mounted read-only there; private root at
  `/private`; existing cached Cargo and identity target volumes at `/cargo` and
  `/target`. No dependency changes.

Initial creation uses these non-secret environment references:

```text
CARGO_HOME=/cargo
CARGO_TARGET_DIR=/target
RUSTUP_TOOLCHAIN=1.94.0
OO_NATIVE_IDENTITY_SERVICE=1
OO_NATIVE_IDENTITY_PRIVATE=/private
OO_NATIVE_IDENTITY_PORT=18447
OO_NATIVE_IDP_PORT=18448
OO_NATIVE_IDENTITY_PROJECT=mysql57
OO_A370_LIVE=1
OO_A370_PG_HOST=oo-dl01w1-71-pg
OO_A370_PG_PORT=5432
OO_A370_PG_DATABASE=oo_mysql57_identity
OO_A370_PG_PASSWORD_FILE=/secrets/pg.password
OO_A370_PG_CA=/pgca/ca.crt
OO_A370_PROOF=/private/proof.json
```

The isolated PostgreSQL uses the existing pinned image and TLS `pg-entry.sh`
recipe, with its own data volume and database. Its network alias
`oo-dl01w1-71-pg` is scoped to the isolated network to match the existing PG TLS
certificate. The existing PG CA and password files are mounted read-only.
Before install, provision the contract's `oo_identity_runtime NOLOGIN` role and
membership of `oo_identity_admin` in this fresh DB server. No binding/session
SQL writes are used. The first attempt omitted this prerequisite and failed;
its private material is preserved separately as `failed-attempt1-private`.
Docker's default address pools were exhausted; the explicit owned subnet avoids
altering existing networks.

## Product API transport

Mount `mysql57-forward.py` into each dedicated product API container and run
`python3 /mounted/path/mysql57-forward.py`. It binds only container loopback
18447 and transports unchanged TLS to `host.docker.internal:18447`. Mount the
identity CA read-only and configure `identity_origin` as
`https://127.0.0.1:18447`. TLS remains verified end to end. Fetch identity/current
freshly for each CSRF-bearing mutation; saved CSRF files are not authoritative.

## Renew and stop

Stop only this owned identity process, then rerun the same container command
with `OO_NATIVE_IDENTITY_RESUME=1` and the same mounts, DB and private directory.
Resume reads the stored issuer port, performs fresh OIDC logins, and replaces
only this fixture's cookie/CSRF files. It skips install/bootstrap and never
reseeds bindings. Sessions last one day; fixture certificates last two days.
A resume does not regenerate expired certificates.

The test holds the actual service open until `/private/stop` exists. To stop
this owned service cleanly, create that file and wait for the test to return;
remove the marker before renewal. Do not stop the old port-18441 service.

Readiness and sanitized role checks are at `private/ready.json` and
`../role-verification.json`. These are fixture prerequisites, not source
acceptance or independent signoff. Source/archive/binary hashes and retained
resource IDs are recorded privately in `../artifact-index.json`.
