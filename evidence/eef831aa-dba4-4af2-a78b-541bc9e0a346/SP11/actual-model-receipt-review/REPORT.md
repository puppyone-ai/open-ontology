# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a346`. Owner a346. This is an independent actual-model receipt review. It is not a new countable acceptance unit. It does not close a parent, the full plan, SP11-W03, RT-D05, Builder 100k, I01, I02, public authentication, or the natural-language harness.

## Verdict

`APPROVE_LIMITED` for one trusted-operator actual natural-language chain.

The request named `grok-4.7`. The saved completion object labels the served model `grok-4.7-build`. The proposal SQL was not rewritten. An operator approval was written after the proposal. `query-pg` returned a clean receipt, an admit and complete audit, one typed integer row `3`, and a bound citation for session `sess-a346` / turn `turn-a346`.

`authenticated` is false. `authenticated_audit` is false. `auto_execute` is false. `model_sql` is `untrusted`. Entailment and warning comprehension are `not_judged`.

## Chain

Question file `linux/propose/question.json`:

```text
How many stored rows are in depot.counter?
```

`question_sha256` `c16ac7264370db3dbb75331a3ebc9478c27a82387e729124e3df056548fe8dfe`.

The observer saved the chat request at `linux/propose/request-body.json` (`request_sha256` `8d35759cb55effde430e4a3423c219ea155d28f95bd42ba47700879047553dd2`). The body sets `model` to `grok-4.7`, `temperature` 0, `stream` false, and the dialogue SQL-candidate schema. Authorization is redacted in `linux/propose/exchange.txt`. HTTP status was 200.

The completion is `linux/propose/completion.json` (`completion_sha256` `02784d6eb7c576d9cdd359bf34edf0d15feca36e2d2c84454f353458ce2a47b6`). One choice, `finish_reason` `stop`. Usage is `prompt_tokens` 1512, `completion_tokens` 157, `total_tokens` 1669. The message `content` is:

```text
{"protocol":"oo.ontology-dialogue.sql-candidate/v1","sql":"SELECT count(*) FROM depot.counter"}
```

The proposal at `linux/propose/proposal.json` keeps that SQL. `sql_sha256` `59ca6630b8b78897caf64197a191d6dd2466427df2ebd84d8de4cf7edb032b36`. `proposal_sha256` `fe7768defcd28378355a193c6cc46350cb3b7233f7a87634ef6dd3fa02d4e268`. Those four hashes were recomputed with `shasum` before approval and recorded in `linux/propose/hash-check.txt`.

Proposal mtime `2026-10-08T01:50:24`. Approval mtime `2026-10-08T01:52:05`. The propose container wrote `approval_written_by_container=false` and `execute_started=false`. The operator decision says `approved=true`, `automatic=false`, `rewritten=false`.

The execute request SQL is the same statement. `linux/execute/execute.stdout` has `query_receipt` `clean`, `child_exit` 0, `query_id` `q_8bd1eadb5903e12d`, `returned_rows` 1, and row `int` `3`. `executed_sha256` is `fbd1d3af68ede73862bb48e12edabcaf890a33a9430c3b8c8a86bc14fffc2755`. The audit has admit and complete, `backend_pid` 100, `backend_start` `20261007175659726193`. The citation receipt is `b78ee58389eca7216fc7f6798404c2fc71fe5a528b83f3e11de7e410f08b5a20`, `citation_status` `ok`, `session_id` `sess-a346`, `turn_id` `turn-a346`.

## Fingerprint and binding

`query_id` and `executed_sha256` follow the statement text. This statement is `SELECT count(*) FROM depot.counter`, so its fingerprint is `q_8bd1eadb5903e12d`.

The reviewed live test in the same container also knows that statement as `COUNT_SQL` and binds its own executions to `sess-a300` / `turn-a300`. Its saved north execution is a different statement, `SELECT headcount FROM depot.counter ORDER BY headcount`, fingerprint `q_3782300da36af286`, proposal `8b2bbc84952f23ff040bacbf4a6c7ed3f2886f8bd6a071e2ff2a82f0099e4a0b`, rows 1, 2, and 3. The live oracle records `count_receipt=clean` for the fixture count case and `north_query_id=q_3782300da36af286` for the headcount case.

This model receipt's proposal is `fe7768defcd28378355a193c6cc46350cb3b7233f7a87634ef6dd3fa02d4e268`, its session is `sess-a346`, its citation receipt is `b78ee58389eca7216fc7f6798404c2fc71fe5a528b83f3e11de7e410f08b5a20`, and its `backend_start` is `20261007175659726193`.

The historical a334 headcount receipt used proposal `6c50244e456458c035fe4b3fd9dbca59f4dc279bae9778436b6f163184c5105b`, fingerprint `q_3782300da36af286`, `executed_sha256` `ed6e42d0833eb27ef9fde69b26604c4c552d9efef9164708188c676937ec9b82`, citation `0a6da4b5572ffa939de8a1d8dd95c114a392a963fd65401731281c115275cc78`, and `backend_start` `20261007164014174293`. The tuple hash `bd23f5d409f27a9c20cda5376b95f2b6d2126b085ffbd2e52df96cf15d030c1c` matches that packet because the tuple line is the role, project, policy, knowledge, as-of, and pack. The question and SQL enter `proposal_sha256`.

## What ran

| Step | Evidence | Exit |
|---|---|---|
| Linux 1.94 smoke before review automation | `linux/smoke` | docker 0, dialogue tests 0, clippy 0 |
| Smoke attempt on a noexec `/tmp` | `linux/smoke/attempt-1` | docker 101, test 101 |
| Host relay archive match and Mach-O build | `host/relay-source-match.txt`, `host/build.exit` | 21 of 21, build 0, offline probe 101, fetch 0 |
| Listen attempt, awk field split | `host/attempt-listen-1` | smoke 1, openssl 0, no listener, no chat |
| Relay, observer, one models GET | `host/smoke.exit`, `host/models-gate.txt` | 0, `grok-4.7` available, no `--chat-out` |
| Observer clippy, then one propose | `linux/propose` | clippy 0, propose 0, docker 0 |
| Live fixture, then one execute | `linux/execute` | live 0, execute 0, docker 0 |

The relay crate at `8c825d8961e93ac44751b6e262cb50ab50afca78` matches `a334` `linux/proof-3/source-after.sha256` for all 21 files. That listing file hashes to `e8fa1f56d315052210e3dc4e1897121960b346f7565940e3328a5f01a1eaed84`. The 28-test campaign was not repeated. Dialogue and `query-pg` came from archive `63566b60aabdf29a29bfe55c4866edf3c4081cfd`, tree `0575fc620e73109f95a19e131e7a1b77f0cf3c33`, dialogue tree `1941b8a6a885ec85ac93da6cc6f4d45c8ecb48a0`. The `query-pg` binary sha256 is `c61aa2631c4f6857e52e910dd8e6a5ad760803367b7470ac8b2e0175d441d281`.

`shasum -a 256 -c` on the a334 packet reported 593 of 593 paths OK. `SHA256SUMS.sha256` is `e79273e76678beb0aafa072eb097492a2daa498863571df5ebdb7013e3e5eb7e`.

Postgres `oo-a346-pg` was 17.10, on `oo-a346-eef831aa-net`, with no host port. Linux containers were unprivileged, capability drop `ALL`, 6 GiB, 1 CPU. The database container was 1 GiB, 1 CPU, with the reviewed capability additions.

## Limits

The host Mach-O binaries were built with Homebrew rustc 1.98.1 because rustup 1.94 is not installed. Linux clippy, propose, and execute used rustc 1.94.0. No cargo update, no lockfile generation, and no root manifest edit. No enterprise call. No auth store was opened. The historical a334 restoration `auth_files_restored=2` and readiness `auth_store_touched=false` stay in that packet.

A byte search of the upstream token file matches only `tools/sha256.rs` and `oracle/observe.rs`. Those files were kept. The match was not printed. The client token and the database passwords matched no file in this tree.
