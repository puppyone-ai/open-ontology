# s3 round of acceptance 5 on rustfs/rustfs 1.0.6 (arm64 and amd64 index digest), run so-v3s-s3-rustfs
$ cargo build --locked -p oo-cli --features docs
     Compiling sqlx-core v0.9.0 (<repo>/crates/oo-connectors/vendor/sqlx-core)
     Compiling rustls-platform-verifier v0.7.0
     Compiling hyper-rustls v0.27.10
     Compiling tiberius v0.13.0
     Compiling reqwest v0.13.5
     Compiling sqlx-postgres v0.9.0 (<repo>/crates/oo-connectors/vendor/sqlx-postgres)
     Compiling oracledb v26.0.0-beta.4
     Compiling oo-sdk v0.1.0 (<repo>/crates/oo-sdk)
     Compiling sqlx v0.9.0
     Compiling oo-connectors v0.1.0 (<repo>/crates/oo-connectors)
     Compiling oo-cli v0.2.1 (<repo>/crates/oo-cli)
      Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 08s
$ cargo build --locked -p oo-docs --features objectstore-s3 --bin oo-docs-engine
     Compiling oo-builder v0.1.0 (<repo>/crates/oo-builder)
     Compiling aws-lc-rs v1.18.1
     Compiling rustls-webpki v0.103.16
     Compiling tokio-rustls v0.26.5
     Compiling sqlx-core v0.9.0 (<repo>/crates/oo-connectors/vendor/sqlx-core)
     Compiling rustls-platform-verifier v0.7.0
     Compiling hyper-rustls v0.27.10
     Compiling reqwest v0.13.5
     Compiling sqlx-postgres v0.9.0 (<repo>/crates/oo-connectors/vendor/sqlx-postgres)
     Compiling sqlx v0.9.0
     Compiling oo-connectors v0.1.0 (<repo>/crates/oo-connectors)
      Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 03s
[PASS] the store is up - rustfs/rustfs
$ docker run -d --name oo-be1-pg-so-v3s-s3-rustfs --label oo.lane=super-ontology-be1 -e POSTGRES_HOST_AUTH_METHOD=trust -p 127.0.0.1::5432 docker.io/library/postgres@sha256:839fbb5a4942a7758dd93e18b42ea5649807491a0545b5eeeb8f0b8bb11857a5 -c track_commit_timestamp=on
  1f9c83ea50ecb037733cccb60e3ffa25f757ab12c4ac669ac6432f54d474c3ff
[PASS] schema installed - {"addr":"127.0.0.1:60788","event":"listening","worker":"engi
$ <repo>/target/debug/oo serve --json --port 0 --state-dir /tmp/oo-s3l-so-v3s-s3-rustfs-6dapk8gy/state
  {"api_version": "1.0", "engine": {"images": {}, "mode": "host_process", "pid": 59121, "project": "host"}, "pid": 59080, "port": 60837, "protocol": "oo.docs.serve/v1", "started_at": "2026-11-18T19:29:52Z", "state_dir": "<state>", "token_file": "<state>/serve.token"}
[PASS] a Drive with an inline key is refused - HTTP 400
[PASS] the s3 Drive is registered - HTTP 201
[PASS] the bystander is published
[PASS] add: published - 4.2s; stored>queued>parsing>parsed>extracting>indexed>published
[PASS] add: evidence is live - 17 rows
[PASS] modify: the two bodies have the same length - 122 bytes
[PASS] modify: new content published - stored>queued>parsing>parsed>extracting>indexed>published>superseded>stored>queued>parsing>parsed>extracting>indexed>published
[PASS] modify: the old version's evidence is withdrawn, the new is live - old 0, new 17
[PASS] rename: the new key has its result, the old is gone - stored>duplicate
[PASS] rename: evidence is live on the new key - 17 rows
[PASS] rename: a file.renamed event exists - 1
[PASS] rename: no fact was retracted - 2 -> 2
[PASS] cache: the original is in the CAS orig/ - 2 object(s)
[PASS] delete: the file disappears
[PASS] delete: the row is marked deleted - true
[PASS] delete: its evidence is withdrawn - 0 live rows
[PASS] delete: facts left without evidence are retracted - 2 -> 6
[PASS] secrets: no key in the stored config, the API or the engine log - secret_ref is a file path

# 20/20 checks passed
