# Super Ontology 形式化验证

> 前提：[00 决策](00-overview-and-decisions.md) D45；状态机与生命周期定义在 00 §5、10 §4、§7；测试分层在 10 §12、20 §6、60。
> 本文规定：哪些东西用形式化方法验证、用什么工具、模型怎样和实现对上、每个闭环要过哪些形式化门禁。

## 1. 为什么要做，做到什么程度

> **D81（2026-11-18）**：本册收窄为低成本一层。签收只看已建成模型与证明的快速检查和一致性测试；`formal-verify-deep` 是可选的每晚检查，不阻断签收；§7 中 V-S7-01、V-S9-01、V-S9-02、V-S11-01、V-S13-01 取消，未完成的 KP 不再补做。下文与本说明冲突处以本说明为准。

Super Ontology 有两类东西光靠测试不够：

1. **并发协议**：作业租约与续跑、文件状态机、对象存储“事件 + 对账”的增量同步、SSE 事件流与断线补齐、批量审核与并发新条目、Share 的刷新与撤销。它们的 bug 只在特定交错下出现，测试很难碰上。
2. **安全承诺**：“共享范围外的内容一个字都不进 Pack”（D26）和“相同输入逐字节可重放”（D27）。这是对用户的承诺，要用证明而不只是用例来支撑。

**做到什么程度**，要说实话：
- 模型检查是**有界**的：在给定规模（例如 3 个 worker、4 个文件、5 次事件）内穷举所有交错。它能找到小规模下的反例，不能证明任意规模都正确。
- Kani 的证明同样是有界的（循环展开上限、输入长度上限）。
- 解析器（Docling）、LLM 的输出质量不在形式化范围内，那由评测集负责。
- 模型对实现的约束靠**轨迹一致性测试**（§4）建立，不是从代码自动提取的。模型和实现之间仍然可能脱节，所以一致性测试是门禁，不是可选项。

## 2. 工具选型

| 工具 | 用途 | 为什么选它 |
|---|---|---|
| [Quint](https://github.com/quint-co/quint) | 为并发协议写可执行规格 | 基于 TLA 的语义，语法接近 TypeScript，带类型检查，对工程师比 TLA+ 原生语法友好；`quint verify` 调用 Apalache 做有界模型检查，`quint run` 做随机模拟；能输出稳定的 ITF 轨迹格式 |
| Apalache（经 `quint verify`） | 有界模型检查，验证不变式 | 符号化检查，状态空间大时比显式枚举更可行 |
| TLC（可选） | 显式状态检查，主要用于验证活性（liveness） | 需要验证“最终一定会到终态”时补充使用；模型经 Quint 编译成 TLA+ 后交给它 |
| [quint-connect](https://github.com/quint-co/quint-connect) | 把 Quint 生成的 ITF 轨迹回放到 Rust 实现上，逐步比较状态 | 提供 `Driver`、`State` trait 和 `#[quint_run]`/`#[quint_test]` 属性，直接生成 Rust 测试 |
| [Kani](https://github.com/model-checking/kani) | 对 Rust 纯函数做有界证明（无 panic、无溢出、函数契约） | `cargo kani` 和 `cargo test` 用法一致；AWS 在 Rust 标准库验证里大规模用于 CI（[论文](https://arxiv.org/abs/2607.01504)）；harness 可以用 `#[cfg_attr(kani, kani::proof)]` 与属性测试共用 |
| proptest / fast-check | 属性测试，覆盖有界证明之外的大规模随机输入 | 已有（10 §12.2、20 §6）；与 Kani 互补 |

不选的方案：Alloy（关系建模很适合可见性，但要再引入一套工具链；可见性模型用 Quint 的纯函数写，规模够用）；P 语言、Coq、Lean（成本和收益不匹配本项目）。

## 3. 验证对象清单

### 3.1 Quint 协议模型（FM）

所有模型放在 `formal/quint/`，每个模型一个文件，旁边放 `*.cfg.md` 写明边界、假设和不变式的人话解释。

| 编号 | 模型 | 对应实现 | 关键不变式（安全性） | 活性（在公平性假设下） | 边界（默认） | 引入 |
|---|---|---|---|---|---|---|
| **FM1** | 作业租约与续跑 `jobs.qnt` | 10 §4.1，`jobs/`、`store/jobs.rs` | 同一幂等键最多一次提交生效；租约过期后旧 worker 的提交被拒绝；重试次数不超过上限，超限即进入隔离；worker 崩溃不会让作业永远卡在“运行中” | 每个作业最终到达终态（完成、失败可重试、隔离） | 3 worker、4 作业、租约过期与崩溃任意交错 | L1 起草、L2 门禁 |
| **FM2** | 文件状态机 `file_lifecycle.qnt` | 00 §5.1、10 §4.2，`state.rs` | 只发生转换表里允许的转换；已发布的事实只来自最新、未删除的文件版本；解析进行中出现新版本时，旧版本的结果不会覆盖新版本；`duplicate` 只指向内容哈希相同的版本 | 没有永久停留的中间态 | 3 文件、每文件 3 个版本、删除与重命名 | L2（md 子集）、L3 全量 |
| **FM3** | 增量同步 `sync_reconcile.qnt` | 10 §4.3，`ingest/` | 事件至少一次投递且乱序时，按 (key, versionId, sequencer) 去重后结果与按真实顺序处理一致；列举期间新写入的对象不会被清扫；部分列举不触发清扫；静止后执行一次对账，索引视图等于桶视图（收敛）；**定向同步（D47）与文件系统监听提示（D46）只能新增或更新、永不清扫**，丢失的监听事件由全量对账兜住 | 静止后最终收敛 | 4 个 key、6 次写或删、事件重复与重排、1 次中断的列举 | L3（fs）、L3（s3）、L7（多云事件） |
| **FM4** | 事件流与断线补齐 `event_stream.qnt` | 10 §8.5，`api/` 的 SSE；20 §1.7 的 store | outbox 序号无间隙、单调；客户端按游标补齐后的视图等于服务端在 head 时的状态，不丢不重；游标过期返回 410 后全量重拉，结果同样一致；瞬时事件（`file.progress`）不影响持久状态；客户端显示的新鲜度（D48）与服务端在同一 seq 下按 00 §5.5 优先级计算的结果一致 | 客户端最终追上 head | 5 个持久事件、2 个瞬时事件、2 次断线、1 次游标过期 | L2 |
| **FM5** | 批量审核 `review_decide.qnt` | 10 §4.8、§8 的 `review:decide` | “按来源整体接受”只作用于 `as_of_seq` 之前出现的条目，之后新进的条目不会被连带接受；`If-Match` 防止丢失更新；同一条目不会被决定两次；D51：只有 LLM 候选的合并与取代永不自动执行；被活跃 Share 使用的事实被取代时一定产生一条待确认 | 每次决定最终反映到新的本体版本 | 2 个来源、5 个条目、并发新条目与并发决定 | L4 |
| **FM6** | Share 生命周期 `share_lifecycle.qnt` | 00 §5.4、10 §7.6–7.8 | `revoked`、`expired`、`refreshed` 的 Share 的 refs 凭证永远无效；refresh 原子地把旧 Share 置为终态并生成恰好一个新的 `active` Share；延期（`PATCH expires_at`）不改变内容与版本；撤销后本地挂载最终被删除；**D50：发布的新版本撤回或取代了某个活跃 Share 用到的事实时，该 Share 最终被标为 `drifted`，且内容不被自动替换**；**D55–D56、D62：包内文件字节不变，`UPDATES.md` 只追加且 seq 无间隙；用过的事实被撤回时一定产生 `block` 条目，`check` 在 block 未处理时永不返回 0；同一任务组内所有活跃 Share 的版本始终相同，组刷新全有或全无；`pinned` 永不刷新，`at-checkpoint` 只在 check 返回 0 或 10 时刷新** | 漂移的 Share 最终被标记 | 2 个 Share、源变化、刷新、撤销、过期、延期任意交错 | L4；L6 扩展漂移与刷新 |
| **FM7** | Suggest 影响式规则 `suggest_rules.qnt` | 10 §11.1 | 四类规则的判定互斥且完备；取代“已被某个 Share 用过”的事实时一定进入人工确认；修改偏好或类型结构一定人工确认 | — | 3 条事实、2 个 Share、4 条建议 | L5 |
| **FM8** | 可见性 `visibility.qnt`（纯函数模型） | D26，10 §7.2 `compile/visible.rs` | Pack 中的每条事实至少有一条证据在共享范围内，且只附范围内的证据；只出现在范围外文件里的实体名、别名，不出现在 Pack 的任何字段里；已拦截计数等于被过滤掉的条目数 | — | 4 个文件、6 个实体、10 条事实、别名跨文件 | L4 |
| **FM9** | 重跑与差异门禁 `reprocess.qnt` | D23，10 §4、B-S10-02 | 重跑期间旧事实持续可用；新结果只在门禁通过后替换；同一时刻不会出现“新旧都缺”的空窗 | 重跑最终完成或回退 | 3 个文件、2 个解析器版本 | L6 |
| **FM10** | 新鲜度令牌与覆盖率 `freshness_token.qnt` | D54，10 §16.2 | `drive_rev` 单调；`covers_drive_rev` 只在所有 ≤ r 的变更到达终态后前进；`require_complete` 的编译只在覆盖满足时成功；非完整编译的 `coverage.pending ∪ failed` 恰好等于范围内未覆盖的文件，且不含范围外文件；纠错（overlay）与文件变更同样推进 `drive_rev` | 静止后 `covers_drive_rev` 追上 `drive_rev` | 3 文件、2 次纠错、并发编译 | L4 |
| **FM11** | 完整性约束 `constraints.qnt` | D57，10 §16.7 | 任一已发布版本中，四条核心约束在任意时间点都成立；违反只阻断涉及的实体，其余变更照常发布；违反中的实体在包里只以“冲突未决”出现；规则确定的取代不被误判为违反 | 冲突经人工决定后最终发布 | 2 个项目、3 个人、并发两条负责人事实 | L4 |

### 3.2 Kani 有界证明（KP）

harness 写在各 crate 内的 `#[cfg(kani)] mod proofs`，函数契约用 `#[kani::requires]`/`#[kani::ensures]`。

| 编号 | 对象 | 证明的性质 | 引入 |
|---|---|---|---|
| **KP1** | 路径规范化（fs 后端、mount 目标、`suggest/` 收集） | 对任意不超过上限长度的输入，输出永远在根目录之内（无 `..` 穿越、无绝对路径逃逸）；不 panic | L2 |
| **KP2** | 锚点的解析与格式化（页 + bbox + 字符区间、`Sheet!A1:C9`、标题路径 + 行号或段落序号） | `parse(format(a)) == a`；非法输入返回错误而不是 panic；单元格区间的行列计算不溢出 | L2 |
| **KP3** | S3 sequencer 比较 | 变长十六进制 sequencer 的比较与“补齐到等长后按数值比较”等价；不按字符串比较 | L3 |
| **KP4** | 重试退避与租约时间运算 | 退避序列单调不减且有上限；时间加法不溢出；尝试次数达到上限时一定返回“隔离” | L3 |
| **KP5** | 可见性过滤函数（在小规模有界图上） | 与 FM8 相同的性质，直接在 Rust 实现上证明：输出 ⊆ 输入，且每条输出事实的证据都在范围内 | L4 |
| **KP6** | BRIEF 排序键与 token 估算 | 排序键是全序（任意两条可比、无并列，保证确定性）；token 估算对拼接满足次可加的上下界；降精度后的 token 数不超过预算 | L4 |
| **KP7** | 状态转换函数 | 对所有（状态，事件）组合都有定义、不 panic，且结果在转换表内（与 FM2 的表逐项一致） | L3 |
| **KP8** | 内容寻址路径计算 | BLAKE3 摘要到目录路径的映射是单射、只产生合法文件名 | L2 |
| **KP9** | 长音频切块的字归属（D73） | 对任意字时间序列与切块边界，每个字恰好归属一块（按时间中点），既不丢也不重；块时间换算回源文件时间轴时加起始偏移不溢出 | L6 |
| **KP10** | AIGC 标识写入（D66） | 对任意 pack id 与文件路径，front matter 与首行标识的生成是确定的、不含时间戳，且写入后正文字节与未加标识时一致（标识只在前缀） | L4 |

## 4. 模型与实现怎样对上：轨迹一致性测试

```
formal/quint/jobs.qnt ──quint run / quint test──▶ ITF 轨迹（JSON）
                                                     │
                     ┌───────────────────────────────┼───────────────────────────────┐
                     ▼                               ▼                               ▼
   Rust：quint-connect Driver               渲染层 store：ITF 回放器          夹具：fixtures/events/
   逐步执行同样的动作，State 比较             （vitest，读 ITF JSON，             （WS-FE2 开发 store 时
   实现状态与模型变量                          applyEvents 后比较视图）            直接使用）
```

- **Rust 侧**：`crates/oo-docs/tests/conformance_<模型>.rs` 用 quint-connect 的 `#[quint_run]` 为 FM1、FM2、FM3、FM5、FM6、FM9 生成测试。`Driver` 把模型动作映射成对实现的调用（用内存 `JobStore`、内存 `DocumentSource` 加故障注入点）；`State` 从实现里提取与模型变量对应的状态并比较。任何一步不一致，测试失败并保存这条轨迹。
- **渲染层**：FM4 的轨迹转换成 `fixtures/events/*.jsonl`，`tests/unit/super-drive/conformance/eventStream.test.ts` 把它们喂给 `superDriveEvents.applyEvents`，逐步比较渲染层视图与模型的客户端视图。不需要额外的库，ITF 就是 JSON。
- **反例回归**：模型检查或一致性测试发现的每一个反例，轨迹都提交到 `formal/traces/regressions/<编号>.itf.json`，并成为永久的回归用例。
- **双向约束**：改动 FM 覆盖的实现，必须在同一个 PR 里更新模型（提交带 `Formal:` trailer，50 §5.3）；模型改了而实现没跟上，一致性测试会失败。

## 5. 流程：模型先于或伴随实现

每个被覆盖的协议，按下面的顺序落地（体现在任务卡里，§7）：

1. **建模**：WS-FV 根据 00、10 的设计写 Quint 模型和不变式，写明边界与假设。
2. **评审**：由该协议的实现板块负责人和一名原生子智能体评审，确认模型和设计一致。评审意见和结论记入 `formal/reviews/<编号>.md`。
3. **检查**：`quint typecheck`；`quint verify`（Apalache，默认边界）；`quint run`（随机模拟，至少 10,000 条轨迹）。发现反例就回到设计，先改 00 或 10，再改模型。
4. **实现**：实现板块按通过检查的模型实现。
5. **一致性测试**：WS-FV 写 Driver/State，接入 CI。
6. **签收**：该闭环的形式化门禁全绿（§8），报告进入证据目录。

模型可以比实现提前一个 Sprint 完成，正好让实现板块“照着模型写”。

## 6. CI 与运行方式

| 作业 | 内容 | 何时跑 | 耗时预算 |
|---|---|---|---|
| `formal-typecheck` | 所有模型 `quint typecheck` | 每个 PR | < 1 分钟 |
| `formal-verify-fast` | 所有模型在默认边界下 `quint verify` | 改动 `formal/` 或被覆盖实现目录的 PR | < 15 分钟 |
| `formal-simulate` | `quint run` 随机模拟，每模型 10,000 条 | 每个 PR | < 10 分钟 |
| `formal-conformance` | Rust 与渲染层的轨迹一致性测试 | 每个 PR | < 10 分钟 |
| `kani-fast` | 改动 crate 的 KP harness | 改动对应 crate 的 PR | < 20 分钟 |
| `formal-verify-deep` | 放大边界（例如 FM1 用 4 worker、6 作业；FM3 用 6 key、10 次写）+ TLC 活性检查 + 全部 KP | 可选，每晚（D81：不阻断签收） | 单独时段，独占机器 |

- 工具版本锁定在 `formal/TOOLCHAIN.md`（Quint、Apalache、Kani、quint-connect 的版本与校验和），经 `supply-chain/` 准入（WS-INT）。
- 为避免 `cargo build` 出现 `unexpected_cfgs` 警告，相关 crate 的 `build.rs` 加 `cargo::rustc-check-cfg=cfg(kani)`。
- Puppyone 侧只有渲染层的 FM4 一致性测试，属于普通 vitest，在 `codex/super-drive` 本地和分支 CI 上跑，不影响 `main`（D43）。

## 7. 形式化任务卡（WS-FV，`V-S<n>-<nn>`）

| 卡 | 目标 | 涉及文件 | 依赖 | 完成定义 | 验证命令 |
|---|---|---|---|---|---|
| V-S1-01 **[主]** | 工具链准入与锁定、CI 作业骨架 | `formal/TOOLCHAIN.md`、`.github/workflows/formal.yml`、`supply-chain/` | — | `formal-typecheck` 在空模型上通过 | `quint --version && cargo kani --version` |
| V-S1-02 | FM1 作业租约模型草稿与评审 | `formal/quint/jobs.qnt`、`formal/reviews/FM1.md` | B-S1-03 | 评审通过；默认边界无反例 | `quint verify formal/quint/jobs.qnt --invariant=inv` |
| V-S2-01 | FM1 一致性测试 | `crates/oo-docs/tests/conformance_jobs.rs` | B-S2-04 | 10,000 条轨迹全部一致 | `cargo test -p oo-docs --test conformance_jobs` |
| V-S2-02 | FM2（md 子集）与 FM4，渲染层 FM4 回放 | `formal/quint/{file_lifecycle,event_stream}.qnt`、`tests/unit/super-drive/conformance/` | B-S2-04、F-S2-05 | 无反例；两侧一致性测试通过 | `quint verify …`；`npx vitest run tests/unit/super-drive/conformance` |
| V-S2-03 | KP1、KP2、KP8 | `crates/oo-docs/src/{ingest,cas}/proofs.rs`、`crates/oo-core/src/docir/proofs.rs` | B-S2-02、B-S2-03 | 证明通过 | `cargo kani -p oo-docs && cargo kani -p oo-core` |
| V-S3-01 | FM2 全量 + KP7 + 一致性测试 | `formal/quint/file_lifecycle.qnt`、`conformance_state.rs` | B-S3-04 | 转换表与 KP7、10 §4.2 逐项一致 | `cargo test -p oo-docs --test conformance_state` |
| V-S3-02 | FM3（fs）+ 一致性测试 | `formal/quint/sync_reconcile.qnt`、`conformance_sync.rs` | B-S2-05 | 收敛性质无反例 | `quint verify formal/quint/sync_reconcile.qnt` |
| V-S4-01 | FM3 扩展到 s3 语义（versionId、sequencer、乱序重复）+ KP3、KP4 | 同上、`ingest/proofs.rs`、`jobs/proofs.rs` | B-S4-02 | 无反例；证明通过 | `cargo kani -p oo-docs` |
| V-S5-01 | FM5 + 一致性测试 | `formal/quint/review_decide.qnt`、`conformance_review.rs` | B-S5-06 | 无反例；一致 | `cargo test -p oo-docs --test conformance_review` |
| V-S5-02 | FM8 可见性模型 + 评审（安全承诺，必须两名评审） | `formal/quint/visibility.qnt` | B-S6-01 设计 | 无反例 | `quint verify formal/quint/visibility.qnt` |
| V-S6-01 | KP5、KP6 | `crates/oo-docs/src/compile/proofs.rs` | B-S6-01、B-S6-02 | 证明通过 | `cargo kani -p oo-docs --harness compile` |
| V-S6-02 | FM6 + 一致性测试；L4 形式化报告 | `formal/quint/share_lifecycle.qnt`、`conformance_share.rs`、`testing/super-ontology/evidence/L4/formal.md` | B-S6-04 | 门禁全绿 | `formal-verify-deep` |
| V-S6-03 | FM10、FM11 模型与一致性测试；FM6 一致性扩展（UPDATES、check、任务组、刷新策略） | `formal/quint/{freshness_token,constraints,share_lifecycle}.qnt`、`conformance_{freshness,constraints,share}.rs` | B-S5-10、B-S5-11、B-S6-11、B-S6-12 | 无反例；一致 | `quint verify …`；`cargo test -p oo-docs --test conformance_freshness --test conformance_constraints --test conformance_share` |
| V-S6-04 | KP10 | `crates/oo-docs/src/compile/proofs.rs` | B-S6-13 | 证明通过 | `cargo kani -p oo-docs --harness aigc` |
| V-S7-01 | FM7 | `formal/quint/suggest_rules.qnt` | B-S7-03 | 互斥完备 | `quint verify formal/quint/suggest_rules.qnt` |
| V-S9-01 | FM6 扩展漂移与刷新、FM9 | `formal/quint/{share_lifecycle,reprocess}.qnt` | B-S10-01、B-S10-02 | 无反例；一致 | `cargo test -p oo-docs --test conformance_share --test conformance_reprocess` |
| V-S9-02 | KP9；FM1 一致性测试扩展到媒体按块续跑 | `tools/media-parser` 的字归属函数移植到 `crates/oo-docs/src/parse/media/chunks.rs` 并加证明；`conformance_jobs.rs` | B-S10-07 | 证明通过；按块续跑的轨迹一致 | `cargo kani -p oo-docs --harness media_chunks && cargo test -p oo-docs --test conformance_jobs` |
| V-S11-01 | FM3 多云事件语义 | `formal/quint/sync_reconcile.qnt` | B-S11-02 | 无反例 | 同上 |
| V-S13-01 | 规模放大后的边界复核与报告 | `formal/reports/L8.md` | B-S13-03 | 深度检查全绿 | `formal-verify-deep` |

## 8. 各闭环的形式化门禁

| 闭环 | 必须全绿 | 报告 |
|---|---|---|
| L1 | 工具链锁定；FM1 草稿评审通过 | — |
| L2 | FM1、FM2（md）、FM4 快速检查无反例；FM1、FM4 一致性测试通过；KP1、KP2、KP8 | `evidence/L2/formal.md` |
| L3 | FM2、FM3 快速检查无反例与一致；已通过的 KP 列出，未完成的如实列为未做 | `evidence/L3/formal.md` |
| L4 | FM5、FM6、FM8、FM10、FM11 快速检查无反例与一致；已通过的 KP 列出 | `evidence/L4/formal.md`，也放进 owner 验收包和 Puppyone 最终 PR 的描述 |
| L5–L8 | 无新增形式化项（D81）；已有模型的快速检查与一致性测试在 CI 中保持全绿 | 各闭环 `formal.md` 一行说明即可 |

每份报告写明：模型版本（commit）、边界、检查的不变式和活性、运行时长、发现的反例及其处理、一致性测试的轨迹数、尚未覆盖的部分。没有覆盖的部分要明说，不写“已形式化验证”这类笼统结论。
