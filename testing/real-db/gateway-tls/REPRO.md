# Repro and deploy notes

This slice is the external TLS listener. It is not an OAuth client, an IdP, or a token issuer. Session issuance and `/internal/*` stay refused.

## Operator file

One directory. `oo-gateway` takes that path as its only argument. `config.json` keys:

Development, including a file that omits `transport`:

```json
{
  "listen": "127.0.0.1:8080",
  "audience": "https://gateway.example.invalid/v1",
  "runtime_user": "oo_token_runtime",
  "password_file": "runtime.pass",
  "ca_file": "pg-ca.crt",
  "endpoint_file": "endpoint.json",
  "pack_dir": "packs",
  "body_limit": 1048576,
  "request_timeout_ms": 5000,
  "global_concurrency": 1
}
```

`transport` may be `"loopback"`. The bound address must be IPv4 `127.0.0.1`.

External clients:

```json
{
  "transport": "tls",
  "listen": "0.0.0.0:8443",
  "audience": "https://gateway.example.invalid/v1",
  "runtime_user": "oo_token_runtime",
  "password_file": "runtime.pass",
  "ca_file": "pg-ca.crt",
  "endpoint_file": "endpoint.json",
  "pack_dir": "packs",
  "body_limit": 1048576,
  "request_timeout_ms": 5000,
  "global_concurrency": 1,
  "tls_key_file": "server.key",
  "tls_chain_file": "server.crt",
  "client_ca_file": "client-ca.crt",
  "handshake_timeout_ms": 400
}
```

Every filename is one path segment in the config directory. `ca_file` is the PostgreSQL server CA. `client_ca_file` is the only trust anchor for client certificates. The key, the chain, and that CA are read before the socket is bound. A bad file exits `1` with `oo-gateway: ` and one of `config`, `listen`, `transport`, `tls_key`, `tls_chain`, `client_ca`, `tls`, `handshake_timeout`. The text does not include a path or PEM. `listen.addr` appears only after bind.

`handshake_timeout_ms` is an integer from 1 through 30000. It bounds one handshake. The HTTP deadline stays `request_timeout_ms`.

## Peer identity

The client certificate must chain to `client_ca_file`. Its leaf must contain exactly one SAN, a URI `urn:oo:workload:` plus 1 to 128 characters from `[A-Za-z0-9_.:-]`. The URI is not a role. Metadata routes still require an `ooct_` bearer. Lookup uses `DeclaredTransport::ExternalClient` and the PostgreSQL row. A forwarded certificate header is `403` on both transports.

## Proof

`linux/proof/launch.sh <attempt>` is the Linux arm64 run. Attempts `proof` through `proof-5` are preserved failures. The passing run is `proof-6`. It does not pull images. Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Secrets stay under `/tmp/oo-a350-eef831aa-dba4-4af2-a78b-541bc9e0a350/secrets` mode `0700` and are not committed. No production credential.
