# L5 Suggest 回流 · OO 侧签收证据（D80 阶段一）

签收版本：OO `feat/super-ontology` @ a54c40586。独立复核运行于 `75c44714b`（分支 `so/V1-signoff`，2026-11-18，验证员与执行者分离），结果 `result-verify-signoff.json`：42 项检查，42 通过、0 失败（L5 验收脚本，加上 `suggest_live`、`review_live`、`correct_api_live` 三个 live 套件）。Desktop 部分（修订模式视图、批量接受界面、`editor` 的文件写入与 Git 自动提交）按 30 §7 在阶段二补齐。

## 30 §L5 验收（OO 侧口径，30 §7.2）

| # | 验收项 | 结果 | 证据 |
|---|---|---|---|
| 1 | 理念文档第 4 节六步完整跑通，第 5 步“点接受”≤ 2 秒 | 通过：`oo context mount --role suggester`、BRIEF 点名负责人且每行带出处、`oo context collect` 收集并改名建议文件、面板出现两条各自取代被 Share 用过的事实的修订（`supersede_in_use`，各列出受影响 Share）、接受耗时 0.017 s（API）/ 0.021 s（CLI，含进程启动），旧 Share 的 `UPDATES.md` 收到广播，`oo context check` 退出 10，下一份 Share 的 BRIEF 显示新负责人与状态。“点接受”的界面点击为阶段二；OO 侧是 API/CLI 的接受 | `result-verify-signoff.json`、`a1-BRIEF-*.md`、`a1-UPDATES.md` |
| 2 | 四类影响式规则各有用例 | 通过：新事实且证据在范围内→自动接受（`new_fact`，并记下写入它的凭证）；取代从未被用过的事实→静默取代（`supersede_unused`）；取代已被 Share 用过的事实→等人确认并列出 Share；改偏好→一律人工（`manual`）；用同一 `idempotency_key` 重发不新增（5→5） | `result-verify-signoff.json`、`a2-digest.json` |
| 3 | 50 条建议的典型一天，人工操作 ≤ 5 次 | 通过：50 条建议中规则自动接受 41 条、等待 11 条，一次 `review:decide` 批量接受 11/11，之后队列为空；一天的人工操作 2 次（读摘要、批量接受），上限 5 | `result-verify-signoff.json`、`a3-human-operations.json` |
| 4 | 每次接受都能在 Git 历史或 overlay 中追溯到建议、Agent 凭证和证据 | overlay 路径通过：43/43 个已接受事实能连到它的建议、写入它的凭证和一个复制的证据块，44 条 `fact_assert` overlay 都点名建议，44 条已发布事实来源为 `suggest`。Git 历史路径（`editor` 角色回写 Drive 文件并经 Puppyone Git 自动提交）为**阶段二**：OO 只提供文件级 diff 草稿（`GET /suggestions/{id}/diff`，`editor` 的 `writable` 为 true、`writer` 为 `desktop`），不直接写 Drive | `a4-trace-sample.txt`、`result-verify-signoff.json` |
| — | OO 不写 Drive（A5） | 通过：运行结束时 Drive 目录 3 个文件逐字节不变 | `l5-result.json`、`result-verify-signoff.json` |

## 横切门禁（L5，OO 侧）

- 形式化：D81 取消了 FM7（建议规则）；规则行为由 `suggest_live` 与 L5 验收脚本覆盖。快速门禁沿用 L4 的完整运行（`../L4/formal.md`），见 `formal.md`。
- 一致性（D55、D56）：MCP 资源 `puppyone://share/<sid>/updates` 订阅通知的 2 s 时限由 `tools/mcp-gateway/tests/updates.rs`（模拟引擎）证明，**不是**由 L5 脚本在真实引擎上证明；`context_check` 的退出码行为在 L4/L5 脚本里验证。

## 已知问题与阶段二事项

- 阶段二（Desktop）：待确认队列的 `suggestion` 种类呈现与按天摘要视图、修订模式视图、“影响哪些正在进行的任务”提示、`editor` 角色的文件写入。
- 这些 live 套件没有持续门禁入口（V2 审计：`suggest_live`、`review_live` 等约 14 个目标），由签收复跑手工执行。
- 独立复核的 `timings` 为空，没有单独的耗时基线；耗时数字来自脚本内的断言。

## 历史记录（已被签收结论取代）

下面是签收前执行者的运行记录（`be2-l5-3`，34 项检查），其中的数字已被上面的独立复核取代。

# L5 Suggest evidence (B-S8-02)

Run `be2-l5-3` of `tools/oo-test/super_ontology_l5.py` (real `oo-docs-engine` under real `oo serve`, labelled PostgreSQL
`postgres@sha256:839fbb5a…`, real `oo context mount|collect`, `oo docs suggestion`), 34 checks, 0 failed. Needs migration 0143
(coordinator patch `B-L5-migration-docs-suggest.patch`) and the docs-api 1.3 contract patch. No token or credential is in
these files.

| Acceptance (30 §L5) | Result | Numbers | Files |
|---|---|---|---|
| 1 the six steps of section 4 of the idea document, accept at most 2 s | PASS | accept 0.021 s (API), 0.026 s (CLI, process start included); the old Share's `UPDATES.md` has the broadcast entry, `oo context check` exits 10; the next Share's BRIEF has 小王 as owner and 跟进中 | `a1-BRIEF-before.md`, `a1-BRIEF-after.md`, `a1-UPDATES.md` |
| 2 the four rules, one case each | PASS | new fact with scope evidence auto accepted; supersede unused silent; supersede used waits and lists the Share; preference waits (manual); resend of an `idempotency_key` adds nothing | `a2-digest.json` |
| 3 50 suggestions in a day, at most 5 human operations | PASS | 2 operations (read the digest, one `review:decide` batch for everything that waits), batch 0.047 s | `a3-human-operations.json` |
| 4 traceability to suggestion, credential, evidence | PASS | 43/43 accepted facts join to their suggestion, the credential that wrote it and a copied evidence block; 44 `fact_assert` overlays name the suggestion | `a4-trace-sample.txt` |
| OO never writes the Drive | PASS | the Drive tree is byte identical at the end | `l5-result.json` |

Not covered here (Desktop, phase two): the revision-mode view (F-S8), the batch-accept UI, the file write for an `editor`
Share (OO provides the diff draft `GET /suggestions/{id}/diff`, `writable` is true for an editor, `writer` is `desktop`).
Notification 2 s bound of `puppyone://share/<sid>/updates` is proved by `tools/mcp-gateway/tests/updates.rs` (mock engine), not
by this script.
