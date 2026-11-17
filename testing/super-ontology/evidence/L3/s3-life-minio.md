# s3 round of acceptance 5 on pgsty/minio RELEASE.2026-08-04T00-00-00Z (community fork of MinIO), run s3l-final-minio
[PASS] the store is up - pgsty/minio
$ docker run -d --name oo-be1-pg-s3l-final-minio --label oo.lane=super-ontology-be1 -e POSTGRES_HOST_AUTH_METHOD=trust -p 127.0.0.1::5432 docker.io/library/postgres@sha256:839fbb5a4942a7758dd93e18b42ea5649807491a0545b5eeeb8f0b8bb11857a5 -c track_commit_timestamp=on
  27af8b6acd8c0ecd0122a0ad58df9b6dbbaa5eeff04da355f3008b80bc7abe12
[PASS] schema installed - {"addr":"127.0.0.1:51079","event":"listening","worker":"engi
$ <repo>/target/debug/oo serve --json --port 0 --state-dir /tmp/oo-s3l-s3l-final-minio-kf2kjf9o/state
  {"api_version": "1.0", "engine": {"images": {}, "mode": "host_process", "pid": 77911, "project": "host"}, "pid": 77910, "port": 51101, "protocol": "oo.docs.serve/v1", "started_at": "2026-11-17T22:05:51Z", "state_dir": "<state>", "token_file": "<state>/serve.token"}
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
