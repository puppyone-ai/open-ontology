# DL01-W1-75 发现（699ca96）

结论与 REVIEW.md 相同：**REQUEST_CHANGES**。完整原退出未达。`d2d93ac` 的六项工程缺陷在新探针里关闭。关闭它们不等于 AT-OOS-04/11 或 SP08-W04 签收。

原料目录 `linux/rework-699ca96/`。上一轮 `linux/probe-steps.json` 与其 ELF 只证明 `193be8e`，本文件不引用它们作为 `699ca96` 的运行结果。

## 已关闭的六项

### C1. G5a fail 不再被 not_applicable 放行

`probe-steps.json` 步骤 `maintain_activate_wide_with_g5a_fail`：code 2，`reason=gate_failed`，`fence_touched=false`，`knowledge_published=false`。计划 G5a `disposition=failed`，`test_admissible=false`。快照 `snapshot_after_g5_fail`：fence closed，指针 null，id 与 contact 均为 42501。维护行 G5a 仍是 `not_applicable` / `control_total_not_supplied`，与 gate_report 的 failed 并存，没有改写 admissibility。

源码：`sql.rs` `READ_BLOCKING_EXTERNAL` 选择 G5a/G5b/G6/G10/G11/G12 的 `fail` 且 `blocking`。`maintain.rs` `activate_locked` 在写 fence 之前 `refuse_before_fence(..., "gate_failed")`。`store.rs` `overlay_one` 在 disposition 已是 `failed` 或 `conflict` 时不把 `test_admissible` 设为 true。

### C2. 纯 not_applicable 保持 test_admissible false

`maintain_activate_legal` 的六门 precondition 与计划门都是 `not_applicable`，`test_admissible=false`，激活 code 0。这是 shop 空输入的 SP08 组件切片，依据是 docs/06 §6 第 575–583 行（没有 certified 指标、verified 示例、评测集或删除名就没有可阻止的失败对象）、spec 第 941 行（本 AT 在 SP08 只到组件发布与 gate fixture）、第 943 行（客户门 pending，不假填）、plan 第 547 行（G10 与会话终止仍 pending）。

它不是这六门通过，也不是作者 fixture 自称的全门通过。已有 blocking fail 时 C1 仍拒绝。

### C3. G7 与打开列、指针列一致

`snapshot_after_legal`：fence open，打开列 `contact,id`，pack 与指针同为 `349b660592d39933061064221839869545e5ddaf0d40a83bbc8d84355dfcbfd1`，policy qual `true`，FORCE RLS true，note 无列权限。G7 detail `open_grant_matches_pointer_columns`，disposition `admissible`。副本 `barrier=missing`，`not_fresh`。没有把缺失副本写成 `replayed`。

### C4. 宽指针与窄 GRANT 时 knowledge_published 为 false

`maintain_recover`：`knowledge_published=false`，`opened_columns=["id"]`，`opened_pack_digest` 为宽 pack，`revoked_columns=["contact"]`，`compiled_pack=not_published`。`snapshot_after_recover`：`SELECT contact` SQLSTATE 42501，`SELECT id` ok。回执的 `outcome` 字面量是 `opened`，发布位是 false。消费方以 `knowledge_published` 为准。

`compiled_pack` 在合法激活、G5 拒绝、并发胜者和 22012 拒绝里都是 `not_published`。

### C5. 调用方 fixture_certificate 不是 admissible

`probe-steps-followup.json` 步骤 `gate_caller_certificate`：code 0，`disposition=foreign_receipt`，`detail=caller_receipt`，`test_admissible=false`。`snapshot_caller` 的 gate_report 相同。issuer 是 `fixture_authority`，report_type 是 `fixture_certificate`，status 是 `pass`，authority hash 来自部署 replay。

主序列 `stage_caller` code 2 `base_mismatch` 是探针用了过期 base `ver-legal`，证书步骤因此改在 followup。这不是产品把证书记成通过。

`decide.rs` `fact_disposition`：`fixture_authority` 只有 `report_type=target_measurement` 且门为 G1/G2/G7/G13 的 pass 才 admissible。`fixture_certificate` 落到 `foreign_receipt`。API `store.rs` `record_intake` 丢弃调用方 authority。HTTP 未重跑，notrun。

### C6. open 失败是 22012 且 fail-closed

宽列故障注入停在 `fence_mismatch` / `column=contact`，因为 contact 已在撤销表。指针未前进，fence 被 revoke 到 closed，权限 42501。这证明撤销列不能再授予，没有覆盖 22012。

窄列 `activate_open_fault_narrow`：code 2，`reason=open_failed`，`open_sqlstate=22012`，`fence_touched=true`，`knowledge_published=false`，`revoke_code=0`，`rollback_outcome=rolled_back`，`pointer_version=ver-race-a`。`snapshot_after_fault_id`：候选 inactive，fence closed，id 与 contact 都是 42501，policy qual `false`。`target.rs` `open_body` 在 `OO_W73_OPEN_FAULT=1` 时执行 `SELECT 1/0`。`maintain.rs` `close_after_open_failure` 做 revoke 和 rollback。

## 仍未达到的原退出

### R1. 完整 G1 与 G9 仍未测量到阻止条件

G1 回执 detail 固定包含 `json_schema_file_not_applied`（`maintain.rs:537`），`test_admissible` 仍为 true。docs G1 包含 JSON Schema。名字、单位和引用的测量不能代替 schema 文件。

G9 是 `not_run`，detail 含 `token_budget_not_measured`。`store.rs` `blocks_test_scope` 让 G9 的 `not_run` 不挡 test_scope。docs G9 的失败处理是阻止。字节上限不是 token 预算通过。

### R2. G5 独立 control_total 与 G10 客户仍缺失

空 shop 的 not_applicable 只覆盖“没有可判对象”。`apply.rs` `preconditions` 在 metrics 非空时把 G5a 标成 `blocked`，detail `certified_metric_without_independent_control_total`。`require_eval` 非空时 G10 为 `blocked`，detail 仍是 `customer_eval_not_arrived`。这条非空路径本轮没有在 Postgres 上跑，notrun。即便源码会拒绝，也没有独立总数或客户回归结果。spec 943 与 plan 547 要求这两项保持 pending。

### R3. 真实副本屏障与 SP11 会话卸载未做

副本 checkpoint 是 `missing` / `not_fresh`。`standby_configured=false`。DEC-05 的 replay 屏障与 G7-post 是 notrun。本轮 `streams_stopped=0`，`sessions_terminated_by_tool=false`。没有持有中的职员会话被工具终止。SP11 保持未验。

### R4. 多项目、多角色仍不支持

`second_project_install` code 2，`reason=project_binding`。`second_role_target_install` 错误文本 `input`。这是单例 fence 拒绝第二绑定。没有第二行 fence，也没有两个角色互相不可读的 SQLSTATE。不能写成隔离通过。原范围不因此缩小。

### R5. Gateway 独立查询未接上

`91031011238c39e571a28a90831b012f225862d6` 存在，主题是公共 Gateway 经 SDK 与 CLI 查询。`699ca96` 的 `tools/release-coordinator` 与 `tools/release-control-api` 源码没有 Gateway 客户端。作者 REPORT 写明本卡没有启动该 Gateway，也没有把 publication 装进它的数据面。本复审没有编译 Gateway，没有发 `oo query`。状态 notrun。原卡的发布知识要由实际 Gateway 读取，这一步未做，不能称 fully closed。

### R6. production_ready 仍是硬编码拒绝

`decide.rs` `production_admissible` 恒为 false。合法 test_scope 回执 `production_blocked=true`，`production_ready_certificate=false`，`customer_ownership_claimed=false`，`full_sp08_accepted=false`。`production_outcome([])` 在空原因时返回 `ready`，活路径会附上 not_production_admissible 等原因。空原因函数不是本轮生产证明。生产外部条件保持 pending。

## 本轮没有重跑、也不能记成通过的项

- 作者 20 项协调器测试、1 项 API 测试、release ELF、尝试 01–11。phase 文件哈希已核对，执行未重放。
- HTTP Control API。调用方证书只走了协调器 `gate_record`。
- 非空 metrics、examples、`require_eval`、删除名的 Postgres 激活。源码会 `blocked`，运行 notrun。
- close、observe、open、enter、commit 的逐步强杀。grant 一步已看到 Lock waiter 并在回滚后保持 closed。
- 作者并发测试里的其他交错。本轮一对 join 的败者是 `cas_conflict` 且 `fence_touched=false`。
- 身份 schema。`identity_schema=not_installed`。
- 整份 source 联结、MCP、DL02。

## 工程消费边界

源实验可以消费 C1–C6 与单角色 fence 的 test_scope 原始行为，包括 42501、列与指针一致才 `knowledge_published`、`compiled_pack=not_published`、调用方证书 `foreign_receipt`。

源实验不能把这次 `test_admissible=true`、`knowledge_published=true` 或副本 checkpoint 当成 G1–G13 完成、G5 control_total、G10 客户、副本屏障或 Gateway 已发布知识。
