# DL01-W1-75 发布维护固定复审（699ca96）

## 结论

**REQUEST_CHANGES。**

按 SP08-W04、DEC-04、DEC-05、REQ-OOS-06/07、AT-OOS-04、AT-OOS-11 和 docs/06 §6 的完整原退出判定。本复审不把该能力标为通过，不晋升 SP08-W04，不改 source 0/10，不改 DL02，不改台账，不改分母。

上一轮复审 `d2d93ac277e45a1eeb586ff22ca8a1220d3d56b9` 对 `193be8e16b2269de7ca5b22341683ad9d92c019b` 的六项工程缺陷，在本轮固定源码 `699ca96df609d739d1887e471c5fb9cbc769ef28` 上已被新探针的原始 JSON 和 SQLSTATE 关闭。关闭的是这六项，整份 AT 仍然未达原退出。

MCP、整份 source、整版发布不在本批准层。Gateway `91031011238c39e571a28a90831b012f225862d6` 的公共 CLI/SDK 已存在。本卡没有把这次 publication 接到该 Gateway 的独立 typed query。这项是 **notrun**，小闭环不能称为 fully closed。

上一轮复审的 raw、失败尝试和 hash 仍留在本目录。`linux/probe-steps.json` 属于 `193be8e` 探针，不能当作 `699ca96` 的执行证明。上一轮 debug ELF `a91047b7146aaea66cce529509d0fd30f3c8b34832b1c8cb712fb03e137db372` 同样只属于那次源码。本轮证据在 `linux/rework-699ca96/`。

## 固定身份

| 项 | 值 |
| --- | --- |
| 被审提交 | `699ca96df609d739d1887e471c5fb9cbc769ef28` |
| 父提交 | `428146a21bcfedf55a8729cb2248b6ed9f86c8a6` |
| 作者 | Redux0223，2026-10-12 09:08:33 +0800，主题 DL01-W1-73 shared ontology publish and maintenance loop |
| 作者路径 | `tools/release-control-api`、`tools/release-coordinator`、`evidence/2f2a2659-9dfd-4387-9f8e-49b87cca7824/DL01/ontology-publish-maintenance` |
| 依赖 pin | `crates/oo-builder`、`oo-core`、`oo-jobs` 与 producer `Cargo.toml` 均为 `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3` |
| producer `Cargo.toml` sha256 | `b772f8fb4873885fcfac747eb24163559031bb9192ba99e8a8472571c75822bc` |
| 工作区选择 | `workspace_manifest=producer_root_9d5df65b`。继承子集回退没有启用 |
| 协调器 lock sha256 | `19daa69f41a5aed2cdf5348e48dad389a5e5bc4ef1bca5273b31604c2b54c5ab`，与 `git show 699ca96:tools/release-coordinator/Cargo.lock` 及作者证据一致 |
| API lock sha256 | `e8ef1c1838d1768112a4abbd4bc8887d524520dec6426a6cd653424b230e76fd`，与 `git show 699ca96:tools/release-control-api/Cargo.lock` 及作者证据一致 |
| 作者源文件清单 | 49 个文件，相对 `git cat-file 699ca96` 的 sha256 差异为 0。清单文件 sha256 `3a17ea6ed1b9be267c9af1f2ad8678abd291330591f0099c54212acafcce75c2` |
| `source-full.sha256` | 文件自身 sha256 `62a00999830d023e5706cdbdb8487099d00fb8aa11cc10a3ccf4e883902d1cc7`。文件内容是清单哈希 `3a17ea6e…` |
| 现场工具树 | 核对时 `git diff 699ca96 -- tools/release-coordinator tools/release-control-api` 为空。编译用的是该提交与 pin 的 git archive。探针 bin 只加在 archive 副本里，不属于 `699ca96` |
| 身份 pin | `tools/control-identity` `a6f92da8a0bf872f8a30efb116c2c7b14463505b` 本轮没有归档。协调器探针不需要它。API HTTP 本轮 notrun |

作者 release ELF 已用 `file` 和 sha256 核对，本轮没有执行它们，也不把它们当作这次源码的运行证明：

- 协调器 `06fa431cec42b7ed03a543574eb526948f6e583e577e7e073355205b80e5577b`，BuildID `c2ae34c0d94885282031fc1fcaa5946d333a2c26`，ARM aarch64 pie，动态链接，未 strip
- API `dd0da4cdd38a95c4bec2bd807089dee914db770436ce2b7112dd93673c97adb2`，BuildID `925d72cfbe729612cf164689c22e200acdcf551b`，同样的 ELF 类

本轮探针 ELF 都是 debug，标签 `probe_elf_class=debug_review_not_author_release`，二进制未入库：

- 主序列与宽列 open-fault：`5459bd6d12a083737e8622f21a4b5b127b76f79c155738bf6327623261201553`，BuildID `5ee5f699e1c203c1213af28c3e3746f71b36f484`
- followup（调用方证书与窄列 22012）：`6aff18aa5b69acbae9dad3d9a440ae7208d424b567428da2167d66fe94c1948b`，BuildID `67a10a377aef9ad7f9df85d0eb3446cf76c8a39b`，149787544 字节

最终探针源 `linux/rework-699ca96/probe/w75_probe.rs` sha256 `f9ed94a485cebcc31df23298fb8f60e5255464bf6d6ee443a3498182b876cab6`。它包含 followup 模式。主序列 JSON 由前一个 ELF 产生，followup JSON 由后一个 ELF 产生。

Rust 镜像 `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`。容器内 `rustc 1.94.0 (4a4ef493e 2026-03-02)`，`cargo 1.94.0 (85eff7c80 2026-01-15)`。Postgres 镜像 `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`，`linux/arm64`。`pg-wrap` sha256 `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`。

## 作者证据核验

`phase-exits.txt` sha256 `eadd43265bcb634a560fff60bdceae3ff6f090aacc249092e3544757a5534b6b`。容器 `oo-dl01w1-73-rust` 2026-10-12T00:58:58Z 至 01:02:12Z，exit 0，oom false，memory `17179869184`。命名阶段均为 0：`coord-fetch`、`coord-clippy-component`、`coord-clippy`、`coord-test`、`coord-build`、`api-fetch`、`api-clippy`、`api-test`、`api-build`。协调器测试 20 passed / 0 failed / 1 filtered，API 测试 1 passed / 0 failed / 1 filtered。作者把 `w73::kill_step_driver`、`w73::publish_maintenance_fence`、`w73_control_api_publishes_one_test_scope_pack` 记为 ok。

这些记录保留。本复审没有重跑这 20+1 项，也没有重编作者 release ELF。滤掉的 `private_postgres_*` 与作者自述的 fixture 通过，都不能替代强制生产退出。

作者尝试 11 `11-rework-clippy-debug-and-if-same` 仍保留：2026-10-12T00:41:54Z 至 00:42:33Z，docker-run exit 101，oom false，`coord-clippy` 101，测试未开始。原因是 `Receipt` 没有 `Debug`，以及 `clippy::if_same_then_else`。尝试 01–10 仍在作者 evidence，本复审未重跑。

## 本复审原料

命名空间 `oo-dl01w1-75`。网络 `10.75.75.0/24`。同一 Postgres 实例两个库：`oo_w75_control`、`oo_w75_target`。身份 schema 未安装。例外在 `linux/rework-699ca96/pg/database-exception.txt`。TLS CA `CN=oo-w75-pg-ca`，服务器 SAN `DNS:oo-dl01w1-75-pg`，客户端 `SslMode::Require`。快照 `ssl=on`。

输入是已批准非生产 shop fixture：项目 `shop`，authority `w73-shop`，角色 `oo_r_clerk`，关系 `public.customer(id int4, contact text, note text)`，一行。宽列 `contact,id`，窄列 `id`。宽 pack `349b660592d39933061064221839869545e5ddaf0d40a83bbc8d84355dfcbfd1`。窄 pack `dbaa88da1ba1d3688070b654bf9f8aa7861dada6c583cfe93d98bf3dee88dbdd`。口令未入库。

`cargo metadata --locked`、`cargo fetch --locked` 之后，`CARGO_NET_OFFLINE=true cargo build --locked --bin w75_probe`。没有 `CARGO_BUILD_JOBS`，NanoCpus 0。Rust 容器内存 17179869184，swap 20g。Postgres 容器内存 2147483648。

引导脚本在 `oo_r_other` 之后创建了 `oo_release_reader NOLOGIN`。没有重启 Postgres。

### 原始 phase exit

| 文件 | 字节 |
| --- | --- |
| `pg/pg-up.exit` | `0\n` |
| `cargo-metadata.exit` | `0\n` |
| `cargo-fetch.exit` | `0\n` |
| `cargo-build.exit` | `0\n` |
| `probe.exit` | `0\n` |
| `probe-open-fault.exit` | `0\n` |
| `probe-followup.exit` | `0\n` |

主容器 `b262e684caebd7d51512903c5a3fcd71d14c6a132b64a907c046f1476567abbe`，2026-10-12T01:29:14.605155428Z 至 01:29:23.87520071Z，exit 0，oom false。followup 容器 `a07290b136da5310b03cb8b98477440d22efcd0d2e2951099c8bf57f0a098bf8`，01:33:36.752540799Z 至 01:33:43.247463927Z，exit 0，oom false。

原文：`probe-steps.json`、`probe-steps-open-fault.json`、`probe-steps-followup.json`。

### 本复审保留失败

| 尝试 | exit | 事实 |
| --- | --- | --- |
| `attempts/01-inside-noexec` | 126 | `bash -c /inside.sh` 在 virtiofs 上执行脚本，`/bin/bash: bad interpreter: Permission denied`。只删该 rust 容器，保留 volume 和 Postgres，改为 `bash /inside.sh` |
| `attempts/02-probe-compile` | 101 | 探针自身 E0596（`holder` 未 mut）和 E0716（`tokio::join` 临时值）。作者源码未改。修正探针后增量编译 |

## 六项原 reproducer

### G5 fail 不能被 not_applicable 放行

`cand-wide` 先写入 G5a `test_issuer` / `fail` / `blocking=true`。`maintain_activate` 返回 code 2，`outcome=refused`，`reason=gate_failed`，`fence_touched=false`，`knowledge_published=false`，`compiled_pack=not_published`。计划里 G5a disposition `failed`，`test_admissible=false`。同单 G5b、G6、G10、G11、G12 为 `not_applicable` 且 `test_admissible=false`，没有把 G5a 改成 admissible。

随后快照：候选仍是 `candidate_built`，指针 version null，fence `closed`，三列 `has_column_privilege` 都是 false，职员 `SELECT id` 与 `SELECT contact` 都是 SQLSTATE 42501。维护行里 G5a 的 detail 仍是 `control_total_not_supplied` / `not_applicable`，与 `gate_report` 的 `failed` 并存。激活看的是 blocking fail，维护行的 not_applicable 没有盖住它。

### 纯 not_applicable 与 shop 激活

合法候选 `cand-legal` / `ver-legal` 没有 metrics、examples、`require_eval`、删除名。激活 code 0，`outcome=activated`。六门 precondition 与计划 disposition 都是 `not_applicable`，`test_admissible=false`：

- G5a、G5b：`control_total_not_supplied`
- G6：`no_verified_examples`
- G10：`customer_eval_not_arrived`
- G11：`no_metric_semantic_diff`
- G12：`no_name_deletion`

`knowledge_published=true`，`compiled_pack=not_published`，`certificate_class=test_scope_not_production`，`production_blocked=true`，`full_sp08_accepted=false`，`standby_configured=false`，`sessions_terminated_by_tool=false`，`streams_stopped=0`。

规范判断见下一节。作者把这条写成工程 fence 可以激活，本复审接受的是原始字段，不接受“门已通过”的自称。

### G7 在打开且列一致之后

合法激活后的快照：fence `open`，`opened_columns` 为 `contact,id`，`opened_pack_digest` 与指针 pack 同为宽 pack，`opened_version=ver-legal`，`candidate_id=cand-legal`。policy `relrowsecurity=true`，`relforcerowsecurity=true`，qual `true`。`oo_r_clerk` 对 id、contact 为 true，对 note 为 false。职员两条 SELECT 的结果字段是 `ok`（单元格数值没有写入 JSON）。G7 行：`pass` / `admissible` / `fixture_authority` / detail `open_grant_matches_pointer_columns`。

副本 checkpoint：`barrier=missing`，`disposition=not_fresh`，`sessions_terminated=false`。fence checkpoint：`barrier=closed`，`disposition=fresh`。没有 `barrier=replayed`。

### 回滚后宽 pack 的 knowledge_published

缩到 `cand-narrow` / `ver-narrow`：code 0，`knowledge_published=true`，打开列只有 `id`，`revoked_columns` 含 `contact`。快照里 `SELECT contact` 为 42501，`SELECT id` 为 ok，指针 pack 是窄 pack。

控制面 rollback code 0，`outcome=rolled_back`，`pointer_version=ver-race-a`（并发胜者，见下），窄候选 state `inactive`。

`maintain_recover` code 0。回执 `knowledge_published=false`，`compiled_pack=not_published`，`certificate_class=not_issued`，`fence_state=open`，`opened_columns=["id"]`，`opened_pack_digest` 是宽 pack `349b6605…`，`revoked_columns=["contact"]`，`outcome` 字段是 status 字面量 `opened`。快照：指针 `cand-race-a` / `ver-race-a` / 宽 pack，fence 打开列只有 `id`，contact 仍在 `revoked_column`，`SELECT contact` 42501，`SELECT id` ok，policy qual `true`。

已撤销列没有恢复。宽 pack 的列集与打开列不一致时，发布位是 false。

### 调用方 fixture certificate

主序列把 `cand-caller` 的 base 写成已经过期的 `ver-legal`，stage code 2，`reason=base_mismatch`，指针当时是 `ver-race-a`。这条没有记入证书。

followup 用当时指针 `ver-race-a` stage `cand-caller2`，再 `gate_record` G1：`fixture_authority` / `fixture_certificate` / `pass` / `blocking=false`，authority hash 来自 install replay。回执 code 0，`disposition=foreign_receipt`，`detail=caller_receipt`，`test_admissible=false`。库中 `gate_report` 同一行：status `pass`，disposition `foreign_receipt`。没有记成 admissible。

API `record_intake` 在 `tools/release-control-api/src/store.rs` 忽略传入的 authority，门和 checkpoint 的 `authority_statement_sha256` 写 None。本轮没有重放 HTTP。HTTP 路径是 notrun。协调器这条调用方证书已经在库里拒绝。

### CAS 之后 open 失败

宽列 followup 之前的 `OO_W73_OPEN_FAULT=1` 跑在 contact 已经撤销之后。`cand-fault` 激活 code 2，`reason=fence_mismatch`，`column=contact`。它没有执行到 `SELECT 1/0`。快照：指针仍是 `ver-race-a`，fence `closed`，打开列残留 `id`，pack digest null，三列权限 false，policy qual `false` 且 FORCE RLS true，职员两条 SELECT 都是 42501，contact 仍撤销。这是撤销列不能再 GRANT 的关闭，不是 22012。

随后用仍可授予的窄列 `cand-fault-id`，同一环境变量，base 为 `ver-race-a`。激活 code 2，`reason=open_failed`，`open_sqlstate=22012`，`knowledge_published=false`，`compiled_pack=not_published`，`fence_state=closed`，`fence_touched=true`，`revoke_code=0`，`rollback_code=0`，`rollback_outcome=rolled_back`，`pointer_version=ver-race-a`。快照：指针仍是 `cand-race-a` 宽 pack，候选 `ver-fault-id` state `inactive`，fence `closed`，三列权限 false，职员 42501，policy qual `false`，contact 仍撤销。副本行仍是 `barrier=missing` / `not_fresh`。

## 并发与 grant 强杀

两个宽候选 `cand-race-a`、`cand-race-b` 以同一 `expected_version=ver-legal` 并发 `maintain_activate`。胜者 code 0，`outcome=activated`，`knowledge_published=true`，`compiled_pack=not_published`，`certificate_class=test_scope_not_production`，`production_blocked=true`，指针 `ver-race-a`。败者 code 2，`outcome=refused`，`reason=cas_conflict`，`fence_touched=false`，`pointer_version=ver-race-a`。赛后快照的 fence、指针和列权限与胜者一致，note 仍无权限。这一对没有出现“先改 fence 再撤销”的分叉。本轮没有重跑作者尝试 05–09。

grant 强杀：父事务 `LOCK TABLE public.customer IN ACCESS EXCLUSIVE MODE`，子进程 `grant_target`。`pg_stat_activity` 看到 1 个 waiter，pid 122，`wait_event_type=Lock`，`state=active`，query 匹配 `LOCK TABLE`。子进程被 kill，`child_code` null。锁回滚后快照仍是 closed、指针 null、三列权限 false。close、observe、open、enter、commit 的逐步强杀本轮没有重跑，状态是 notrun，不记成通过。

`streams_stopped=0` 是因为激活时没有持有职员会话。这不证明 SP11 会话卸载。

## 纯 not_applicable 按哪条原规范

shop 空 metrics、空 examples、空 `require_eval`、空删除名时，G5a、G5b、G6、G10、G11、G12 为 `not_applicable` 且 `test_admissible=false`，单角色 fence 仍可激活。这符合下面三条，同时停在它们的边界上。

docs/06 §6：G5a 阻止的是 certified 指标；G5b 阻止的是 `control_total` 与指标结果不一致；G6 阻止的是 verified 示例执行失败；G10 阻止的是 `require_eval` 评测回归；G11、G12 阻止的是未审核的语义差异和不合生命周期的删除。这份 fixture 里没有 certified 指标、没有 verified 示例、没有评测集、没有删除名，所以没有一条可执行的失败对象。`not_applicable` 加上 `test_admissible=false` 表示“没有可判对象，也没有判成通过”。

spec 第 941 行：`AT-OOS-04/11@SP08` 只覆盖组件发布与 gate fixture，真实会话收缩在 SP11，G10 客户回归在 SP12。部分验收不得把全 AT 标成 passed。第 943 行：客户特定 gate 标 pending，不假填默认值。

plan SP08-W04 签收（第 547 行）：每 gate 正负 fixture、真实权限副本屏障、实体守恒与指标独立总数仍是阶段签收条件。真实会话终止在 SP11 再验，客户 holdout/G10 在 SP12 再验，未验部分保持 pending。

因此：shop 工程 fence 可以在这六门 `test_admissible=false` 的前提下激活，作为 SP08 的组件 fixture 切片。它不是 G5 独立总数通过，不是 G10 客户通过，不是生产通过，也不是 G1–G13 完成。已有 G5a `fail`+`blocking` 时，同一套 not_applicable 维护行不能放行，本轮 code 2 已覆盖这条。

`apply.rs` 的 `preconditions`：metrics、examples、eval、删除名非空时 disposition 为 `blocked`。G5a 的 blocked detail 是 `certified_metric_without_independent_control_total`。这条非空路径本轮没有在 Postgres 上重跑，运行状态 notrun。源码与作者单测在，不能写成已有客户或独立总数证据。

## 逐项原范围

| 项 | 本轮范围 | 判定 |
| --- | --- | --- |
| DEC-04 收缩后旧授权不可继续 | 缩列、rollback、recover 后 contact 保持 42501，宽指针下 `knowledge_published=false` | 工程项达到。会话卸载仍是 SP11 |
| DEC-05 副本屏障后才解封 | 副本 `barrier=missing` / `not_fresh`，`standby_configured=false` | 诚实缺失。真实 standby notrun。原退出未达 |
| REQ-OOS-06 结构化 gate_report | 库内有 gate_id、status、blocking、disposition、detail | 有报告。G1 JSON Schema 文件未应用仍被 test_admissible。见下 |
| REQ-OOS-07 / docs G1–G13 | 见下表 | 完整门未达 |
| AT-OOS-04 / AT-OOS-11 @ SP08 | 单角色组件发布与 gate fixture | 切片内的 fence、CAS、撤销、证书拒绝达到。全 AT 不通过 |
| SP08-W04 签收 | 正负 fixture、副本屏障、实体守恒、独立总数 | 未达，保持 pending |
| 多项目 / 多角色隔离 | 第二项目 code 2 `project_binding`；第二角色 `install_target` 错误 `input` | 不支持。缺失，不是隔离证明 |
| Gateway 独立 query | 699ca96 的协调器与 API 源码没有 Gateway 客户端。作者 REPORT 写明没有启动 910310，也没有把 publication 装进数据面 | notrun。不能称 fully closed |
| production_ready / 客户 / G5 control_total | 回执 `production_blocked=true`，`production_ready_certificate=false`，`customer_gate_verified=false` | 外部条件缺失，保持 pending |
| MCP / 整源 / 整版 | 不在本批准层 | 不批准 |

G1：测量覆盖唯一名、money unit、引用（`apply.rs` `measure_g1`）。写入 detail 固定带 `json_schema_file_not_applied`（`maintain.rs` 第 537 行），同时 `test_admissible=true`。JSON Schema 文件没有应用。完整 G1 未达。

G2：本轮 shop 激活为 `catalog_type_match` / admissible。这是这份 fixture 的目录类型匹配，不是全部 catalog 漂移矩阵。

G3、G4：docs 把它们写成警告。本轮 `not_run`，`test_admissible=false`，detail 为 `containment_not_measured` 与 `enum_coverage_not_measured`。没有包含度或覆盖度结果，也没有被记成通过。

G5a、G5b、G6、G10、G11、G12：空 fixture 为 not_applicable 且不可作为通过。已有 G5a blocking fail 会在写 fence 之前拒绝。独立 control_total 与客户评测仍缺失。

G7：本轮 pass 发生在目标 open 且 GRANT 列与指针列一致之后。G7-post 副本屏障没有做。

G8：`seal.rs` `recompile_g8` 核对 pack digest，并要求 manifest 声明 `samples=omitted`、`publication=not_claimed`。回执 `role_pack_samples_omitted` / admissible。本轮没有再扫 pack 文件正文里的样本值。shop 的 pii 是 none。这不是生产 G8。

G9：detail `byte_length_checked;token_budget_not_measured`，status `not_run`，`test_admissible=false`。`blocks_test_scope` 对 G9 的 `not_run` 不挡激活。docs G9 是阻止。token 预算未测，不能称为通过。

G13：`classification_and_explicit_role` / admissible。shop 分类不是 personal/sensitive 的客户矩阵。

`compiled_pack` 在成功、拒绝和故障路径上都是字面量 `not_published`。`knowledge_published` 只在 fence open、指针指向该候选、打开列与可见列一致且可见列未被撤销时为 true。

## 工程功能与外部条件

可以给后续源实验当作 test_scope 消费的，只有下面这条边界内的机制：单角色单表的 REVOKE、FORCE RLS、列权限和 42501；G5a blocking fail 在写 fence 之前拒绝；纯 not_applicable 保持 `test_admissible=false`；调用方 `fixture_certificate` 为 `foreign_receipt`；`compiled_pack` 保持 `not_published`；`knowledge_published` 与打开列、指针可见列一致；CAS 冲突不改败者 fence；open 在可授予列上注入 22012 时 fail-closed 并 rollback。

不能消费成原退出的：G1–G13 完成、G5 独立 control_total、G10 客户、G7-post 副本屏障、production_ready、多项目多角色隔离、SP11 会话卸载、Gateway 独立 query、整份 source。`test_admissible=true` 的 G1、G2、G7、G8、G13 只表示这条 shop test_scope 测量，不表示那些原门已关闭。

## 清理

只删除 `oo-dl01w1-75-rust`、`oo-dl01w1-75-rust2`、`oo-dl01w1-75-pg`，卷 `oo-dl01w1-75-cargo`、`oo-dl01w1-75-target`、`oo-dl01w1-75-pg`，网络 `oo-dl01w1-75`，以及 `/tmp/oo-dl01w1-75`（含口令）。`foreign_volumes_not_deleted=true`。`historical_containers_not_restarted=true`。清理后 `oo-dl01w1-74-rust` 与 `oo-dl01w1-74-pg` 仍是 running。回执 `linux/rework-699ca96/cleanup-receipt.txt`。ELF 与口令未入库。
