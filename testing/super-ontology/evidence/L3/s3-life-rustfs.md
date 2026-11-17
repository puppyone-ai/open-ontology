# s3 round of acceptance 5 on rustfs/rustfs 1.0.6 (arm64 and amd64 index digest), run s4-l2s3-rustfs
$ cargo build --locked -p oo-cli --features docs
  Compiling sqlx-core v0.9.0 (<repo>/crates/oo-connectors/vendor/sqlx-core)
     Compiling oo-core v0.1.0 (<repo>/crates/oo-core)
     Compiling oo-stack v0.1.0 (<repo>/crates/oo-stack)
     Compiling sqlx-postgres v0.9.0 (<repo>/crates/oo-connectors/vendor/sqlx-postgres)
     Compiling sqlx v0.9.0
     Compiling oo-connectors v0.1.0 (<repo>/crates/oo-connectors)
     Compiling oo-sdk v0.1.0 (<repo>/crates/oo-sdk)
     Compiling oo-cli v0.2.1 (<repo>/crates/oo-cli)
      Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.52s
$ cargo build --locked -p oo-docs --features objectstore-s3 --bin oo-docs-engine
  Compiling oo-docs v0.1.0 (<repo>/crates/oo-docs)
      Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.71s
[PASS] the store is up - rustfs/rustfs
$ docker run -d --name oo-be1-pg-s4-l2s3-rustfs --label oo.lane=super-ontology-be1 -e POSTGRES_HOST_AUTH_METHOD=trust -p 127.0.0.1::5432 docker.io/library/postgres@sha256:839fbb5a4942a7758dd93e18b42ea5649807491a0545b5eeeb8f0b8bb11857a5 -c track_commit_timestamp=on
  9c7db442bf3cc6a0a66801771f820a1cfd978d473afd4b6168bdd32024fbf3ac
[PASS] schema installed - {"addr":"127.0.0.1:59176","event":"listening","worker":"engi
$ <repo>/target/debug/oo serve --json --port 0 --state-dir /tmp/oo-s3l-s4-l2s3-rustfs-c2j84_1f/state
  {"api_version": "1.0", "engine": {"images": {}, "mode": "host_process", "pid": 6613, "project": "host"}, "pid": 6605, "port": 59326, "protocol": "oo.docs.serve/v1", "started_at": "2026-11-17T22:16:21Z", "state_dir": "<state>", "token_file": "<state>/serve.token"}
[PASS] a Drive with an inline key is refused - HTTP 400
[PASS] the s3 Drive is registered - HTTP 201
[PASS] the bystander is published
[PASS] add: published - 6.1s; stored>queued>parsing>parsed>extracting>indexed>published
[PASS] add: evidence is live - 15 rows
[PASS] modify: the two bodies have the same length - 122 bytes
[PASS] modify: new content published - stored>queued>parsing>parsed>extracting>indexed>published>superseded>stored>queued>parsing>parsed>extracting>indexed>published
[PASS] modify: the old version's evidence is withdrawn, the new is live - old 0, new 15
[PASS] rename: the new key has its result, the old is gone - stored>duplicate
[PASS] rename: evidence is live on the new key - 15 rows
[PASS] rename: a file.renamed event exists - 1
[PASS] rename: no fact was retracted - 1 -> 1
[PASS] cache: the original is in the CAS orig/ - 2 object(s)
[PASS] delete: the file disappears
[PASS] delete: the row is marked deleted - true
[PASS] delete: its evidence is withdrawn - 0 live rows
[PASS] delete: facts left without evidence are retracted - 1 -> 4
[PASS] secrets: no key in the stored config, the API or the engine log - secret_ref is a file path

# 20/20 checks passed
