# DL01-W1-73 发布维护返工包（针对 699ca96 复审）

交给原 73 工程师的一份返工。不拆新卡，不新编分母，不改台账。复审结论仍是 REQUEST_CHANGES。对象仍是 SP08-W04、DEC-04、DEC-05、REQ-OOS-06/07、AT-OOS-04、AT-OOS-11、docs/06 §6 的原退出。

固定实现已经是 `699ca96df609d739d1887e471c5fb9cbc769ef28` 的 `tools/release-coordinator` 与 `tools/release-control-api`。本轮原料在 `evidence/67a28659-717e-417f-a26b-2f981ae5c64c/DL01/publish-maintenance-fixed-review/linux/rework-699ca96/`。上一轮 `d2d93ac` 的 raw 保留，不要把它当成这份源码的失败复现。

## 已经关闭、不要再当作未修缺陷

下面六项有本轮原始 JSON 和 SQLSTATE。返工不要把它们重新打开成“仍然绕过”：

1. G5a `fail`+`blocking` 的 `maintain_activate` 为 code 2，`reason=gate_failed`，`fence_touched=false`，G5a `test_admissible=false`。维护行的 `not_applicable` 没有盖住这条 failed。
2. 空 shop 的 G5a、G5b、G6、G10、G11、G12 保持 `not_applicable` 且 `test_admissible=false`。这符合 spec 941 的 SP08 组件切片和 docs/06 里“没有 certified 指标 / verified 示例 / 评测集 / 删除名”。它不是这六门通过。
3. G7 pass 出现在 fence open、policy qual `true`、GRANT 列与指针列同为 `contact,id` 之后。detail `open_grant_matches_pointer_columns`。无 standby 时副本是 `barrier=missing`，不是 `replayed`。
4. rollback / recover 后 contact 仍是 42501。指针回到宽 pack 而打开列只有 `id` 时，`knowledge_published=false`，`compiled_pack=not_published`。
5. 调用方 `fixture_authority` / `fixture_certificate` / `pass` 的 disposition 是 `foreign_receipt`，detail `caller_receipt`，`test_admissible=false`。
6. 可授予列上 `OO_W73_OPEN_FAULT=1` 的 open 返回 code 2，`open_sqlstate=22012`，`reason=open_failed`，revoke 与 rollback 都是 code 0，指针回到故障前版本，职员 SELECT 为 42501。已经撤销的 contact 上再激活宽列会先 `fence_mismatch`，这是撤销保持，不是 22012 缺失。

单角色 fence、一次并发败者 `cas_conflict` 且 `fence_touched=false`、一次 grant 步 Lock 强杀，也已经有原始记录。不要为了这三项再改出另一套接口。

## 接口

继续用现有公开面：

- `maintain_install` / `maintain_activate` / `maintain_recover`
- `gate_record` / `stage` / `status` / `recover`
- target：`install_target`、`close_target`、`grant_target`、`observe_target`、`open_target`、`revoke_target`、`status_target`
- 控制指针与 `oo_publish_target.fence` 的打开列、pack、撤销表
- 发布知识的读取方是固定 Gateway `91031011238c39e571a28a90831b012f225862d6` 的公共 CLI/SDK。不编译该目录的 dirty 或 live 树

`compiled_pack` 继续保持字面量 `not_published`。`knowledge_published` 继续要求 fence open、指针指向该候选、打开列等于指针可见列、可见列没有撤销项。CLI 继续拒绝 force 和未知配置键。

## 输入

沿用已批准非生产 shop fixture，与本轮探针相同：项目 `shop`，authority `w73-shop`，角色 `oo_r_clerk`，`public.customer(id int4, contact text, note text)`，一行 `(1, ada@example.test, hidden)`，宽列 `contact,id`，窄列 `id`。

另外保留四类非空 effective：有 metrics、有 examples、有 `ontology.publish.require_eval`、有删除名。Linux Rust 1.94 与本轮同一镜像 digest。TLS Postgres 17 同一镜像，`pg-wrap` sha256 `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`。一个实例两个库可以继续作为记录在案的例外。身份 schema 不必为这条返工安装。没有 `OO_W73_REQUIRED` 的 `cargo test` 不是这份退出的证明。

Gateway 查询使用该固定提交的 archive，输入是这次 publication 的角色、列和版本，不使用未发布 fixture 冒充已发布知识。

## 必须一起达到的退出

1. G1 的 test_admissible 只有在 JSON Schema 文件真正应用之后才能为 true。现在的 detail 固定写 `json_schema_file_not_applied` 同时 admissible（`maintain.rs:537`）。schema 文件未用时，G1 保持失败或 not_run，并挡住把该门当成通过的计划。唯一名、money unit、引用可以继续作为 G1 的其余条款。

2. G9 的 token 预算要有测量。未测量时不得借 `blocks_test_scope` 对 `not_run` 放行后称为预算通过。docs G9 的失败处理是阻止。字节长度检查保持为字节检查。

3. 空 shop 可以继续 `not_applicable` 且 `test_admissible=false`，并激活单角色 test_scope fence。有 certified 指标时 G5a/G5b 保持 blocked，直到一条独立 `control_total` 在锚点上与指标结果一致。没有这条总数之前，不得把 G5 写成通过，也不得把 shop 的 not_applicable 写成 control_total 已到。非空 metrics 的 Postgres 激活本轮是 notrun，返工要留下这条原始拒绝或真实对账。

4. G10 保持 SP12 客户回归。`require_eval` 非空时继续 blocked，detail 不得填成客户分数。fixture 名称 `customer_eval_not_arrived` 不是客户结果。spec 943 与 plan 547 的 pending 保持。

5. 没有真实 standby 时，副本 checkpoint 继续 `barrier=missing`、`disposition=not_fresh`、`standby_configured=false`。要声称 DEC-05 屏障，需要真实副本 replay 与 G7-post，原始行里 barrier 才能是 `replayed`。禁止手写 replayed。

6. `sessions_terminated_by_tool` 继续 false，直到 SP11 的真实会话卸载。本轮 `streams_stopped=0` 是因为没有持有中的读者。不要为了回执数字去写 `sessions_terminated=true`。

7. 第二项目 `project_binding` 与第二角色 `input` 继续表示不支持。文档和测试不得写成多角色隔离。要满足原范围里的多项目多角色，需要第二行 fence，以及两个角色互相读不到对方列的 SQLSTATE。在那之前，发布回执保持单例边界，`full_sp08_accepted=false`。

8. 用固定 Gateway `910310…` 的 `oo query`（以及该提交已经证明的 freshness / cancel / typed JSON）读取这次 test_scope publication 的角色、可见列和版本。查询结果与指针列一致：宽发布能读到 id 与 contact、读不到 note；缩列后 contact 不可读。没有这条独立客户端原始输出之前，发布维护小闭环保持未闭合。不编译 dirty Gateway，不伪造一条成功查询。

9. `production_admissible` 保持 false。`production_ready_certificate`、`customer_ownership_claimed`、`customer_gate_verified`、`standby_configured`、`full_sp08_accepted` 保持 false，直到对应外部证据存在。`production_outcome` 的空原因返回 `ready` 不得被任何活路径当成生产证书。

## 测试与原始退出

同一条 Linux Rust 1.94、真实 TLS Postgres 运行里保留本轮已经绿的六项，并新增 1–8 的原始 stdout 或 SQLSTATE。至少包括：

- 无 JSON Schema 文件时 G1 的 disposition 与 `test_admissible`
- 无 token 预算测量时 G9 是否还允许激活，以及回执 detail
- 一份含 certified 指标且没有独立 control_total 的候选：激活 code、reason、fence_touched
- 一份 `require_eval` 非空的候选：disposition 仍不是客户分数
- 副本表里不存在 `barrier=replayed`，除非真有 standby
- 第二项目与第二角色的拒绝原文；若实现了第二 fence，则交叉 SELECT 的 SQLSTATE
- Gateway `oo query` 的客户端、版本、角色、列和与指针一致的结果或拒绝
- 全程 `compiled_pack=not_published`，`certificate_class` 不到生产证书

作者现有的 20 项协调器测试和 1 项 API 测试可以保留。它们不替代上面的 Gateway 查询、客户门和独立总数。

## 本复审边界

本轮探针是 debug ELF，主序列 `5459bd6d12a083737e8622f21a4b5b127b76f79c155738bf6327623261201553`，followup `6aff18aa5b69acbae9dad3d9a440ae7208d424b567428da2167d66fe94c1948b`。都不是作者 release ELF。作者 release ELF 与其绿测试未重建。返工不需要为了反驳本复审去重编那两个 release 二进制。需要的是上面 1–9 的新原始退出，与已经关闭的六项放在同一次完整证明里。
