# 03 Self-build 上下文预算：动态分配输入输出，产出连通的 Ontology

> Status: Proposed（§2 现状为已实现事实）
> Owner: Open Ontology Engineering
> Last reviewed: 2026-11-03
> 依赖：[06 Ontology 层](../06-ontology-layer.md)、[04 onboarding](04-onboarding-and-gateway-binding.md) §4.4

## 1. 问题

生产库有几百到几千张表、上万列。任何模型的上下文窗口都装不下整库，输出上限更小。简单按表数切批会出两类问题：

1. **输入或输出溢出**：批内列多时输出被截断（`finish_reason=length`），JSON 不完整；
2. **关系断裂**：每批只看到局部，跨批的外键、共享维度、同一实体的多张表被当成互不相关，产出的 Ontology 是一堆孤岛。

## 2. 现状

| 项 | 实现 | 位置 |
|---|---|---|
| 批切分 | 每批最多 16 表、240,000 字节，超出递归对半拆 | `tools/pg-builder/scripts/library_selfbuild.py` `draft_batches` |
| 输出上限 | `min(40000, max(4000, 3500 + 列数 × 70))` | 同上 |
| Rust Builder 限制 | `MAX_TABLES=64`、`MAX_COLUMNS=256`；列每 8 个一批 | `crates/oo-builder/src/limits.rs`、`tools/pg-builder/src/generate.rs` |
| token 估算 | 按词数近似，不用模型 tokenizer | `crates/oo-builder/src/knowledge_budget.rs` |
| 事实输入 | S0 精确统计、S3 包含度关系检查；不发原始行 | `library_selfbuild.py` |
| 参考规模 | 658 表 / 15,792 列 → 71 批，输入约 237 万 token、输出约 35 万 token | 参考生产部署 |

Python 路线已经能跑通几百表规模；Rust 路线还受 64 表限制。v0.2 统一到下文的预算器，并移除项目级表数上限。

## 3. 预算器

### 3.1 模型能力探测

首次调用时探测并记录：上下文窗口 `W`、最大输出 `O_max`、是否支持 JSON 模式/结构化输出、是否返回 usage。用户可在 `stack.yaml` 覆盖。探测失败时用保守默认值（`W=32k`、`O_max=4k`）并在预检中提示。

### 3.2 每次调用的分配

```text
I_avail = W − O_reserve − P_sys − margin
margin  = max(2k, 0.1 × W)
O_reserve = clamp(base + Σ per_item_out, O_min, O_max)
```

- `per_item_out` 按要产出的条目估：每列约 70 token、每表约 250 token、每候选关系约 60 token；这些系数在运行中用实际 `usage.completion_tokens` 做 EWMA 校准。
- 批的约束是**双向**的：既要 `Σ input ≤ I_avail`，也要 `O_reserve ≤ O_max`。列很多、描述很长的表先触发输出约束，此时按列分片（同一张表拆成多段列描述），表级字段只在第一段产出。
- token 计数：provider 有计数接口时用之；否则按字节保守估计（中文按 1 字节/token 的上界、ASCII 按 3 字节/token），并用首批真实 usage 校准比例。

### 3.3 超预算时的降级顺序

单批仍超预算时按顺序降级，每一步都记录在批次回执里：减少样本值 → 列压缩为“名称:类型:关键统计” → 按列分片 → 递归切社区。不丢表、不丢列。

### 3.4 截断与修复

- 输出用 NDJSON 按条目逐行返回（每行一个表/列/实体/关系），每条带稳定 ID（物理表名/列名派生）。截断时只丢最后一条不完整记录，比修补半个 JSON 文档可靠。
- 遇到截断或校验失败：计算缺失条目 ID 列表，**只重新请求缺失项**，附带有界的校验错误反馈；不要求模型“继续上文”。参考部署中某批连续三次漏掉 4 个字段，原样重发无效，带反馈重生成完整 JSON 才解决。
- 程序不代补模型缺失的语义内容；补不齐的条目标记 `unresolved` 进入审核。

### 3.5 并发与额度

- 并发从 1 起步，连续成功后升到 2；出现不完整流立即降回 1（参考部署 4 路并发时流不完整）。
- `usage_limit`、`cooldown`、429 统一 fail-fast：停止派发新批次，保留已完成批次回执，下次 `--resume` 续跑。
- 批次回执以 `(model_id, prompt_template_hash, input_hash)` 为键；换模型后只复用同一模型的回执。
- 流式响应以 finish 帧为准，不依赖 `[DONE]`。

## 4. 保持关系连通

### 4.1 先建图，再切批

0. **确定性预处理（不调模型）**：按名字模式折叠分表（如 `order_2023_*`、`log_yyyymm`），折叠审计、临时、备份表；只把一个代表表送进模型，其余作为同族成员继承语义并在 Pack 中列出。

1. **关系图**：节点是表；边来自声明外键、S1 包含度检查得到的S3 候选关系（带置信度）、命名约定（`*_id`、`*_code`）。
2. **社区划分**：在关系图上做社区检测（Leiden/Louvain），社区大小受 §3 预算约束；超出预算的社区递归切分，切分时最小化被切断的边权。
3. **枢纽表**：度数高的表（组织、客户、商品、日历、码表）不归入任何单一社区，而是以**紧凑签名**（表名、主键、10 个最重要列、实体名）作为只读上下文附加到所有引用它的批次。

### 4.2 三遍生成

| 遍 | 输入 | 产出 |
|---|---|---|
| 第 1 遍：社区内 | 一个社区的表与事实 + 枢纽签名 + 当前全局实体登记表摘要 | 表/列描述、实体、社区内关系、术语、指标候选 |
| 第 2 遍：跨社区 | 只含被切断的边：两端表的签名与相关列事实 | 跨社区关系的确认、方向、基数、业务名 |
| 第 3 遍：合并 | 全局实体登记表 + 关系清单 + 冲突列表 | 实体去重（同一业务实体的多张表）、命名统一、冲突裁决理由 |

**全局实体登记表**：每个实体有稳定 ID、规范名、别名、主表、关联表。第 1 遍每完成一个社区就更新，后续批次拿到的是它的压缩摘要（只有 ID、规范名、主键形态），所以后面的批次能复用已有实体而不是另起新名。

### 4.3 连通性门禁

发布前检查并写入审核摘要：

- 所有声明外键都映射为 Ontology 关系，或有拒绝理由；
- 孤立实体（无任何关系）占比，超过阈值进入审核重点；
- 实体图连通分量数，以及从核心实体出发 3 跳内可达的表占比；
- 同名不同义、同义不同名的冲突数及裁决状态。

这些指标是审核输入，不是自动通过条件；模型不能自己给指标“认证”。

**层级摘要**：列 → 表卡片 → 社区摘要 → 领域摘要，上层只读下层摘要。领域摘要进入 Pack 的 README/INDEX，作为 Agent 的第一入口。

## 5. 增量：只重算受影响的部分

- 每张表、每个社区有语义依赖哈希（结构、类型、关系候选、相关枢纽签名）。
- 数据-only 周期：哈希不变，零模型调用，只更新数据与统计事实。
- 新增列、改类型：只重算该表所在社区的第 1 遍、相关跨社区边的第 2 遍、合并遍的受影响实体。
- 统计事实摘要按社区划分，不再用全局摘要（全局摘要一变会让全部批次失效）。

## 6. Knowledge Pack

Pack 继续由 `crates/oo-builder/src/role_pack.rs` 渲染，文件布局以 [06 Ontology 层](../06-ontology-layer.md) 为准，新增：

- `entities/<id>.md` 中列出所有关联表与跨社区关系，作为 Agent 的导航入口；
- `relationships/graph.json`：机器可读的实体关系图（与 06 的 `relationships` 页面并存），同时是可视化导出（`oo ontology export --format html|mermaid`）的输入；
- 每个文件有预算，超出时分页而不是截断，INDEX 指向分页。

## 7. 费用与时间预估

预检阶段用 §3 的公式对整库做一次“干跑”切分，输出：批数、预计输入/输出 token、按 provider 单价折算的费用区间、按当前并发估计的耗时。用户确认后才开始调用模型。

## 7.1 S3 实测（2026-11-04）

`g-s3-thousand-20261104/live-04`：PG 17.11 经 oo-load 装载 1,033 表、2,391 列；19 个社区均在规划预算内，7/7 声明外键映射，零 unresolved。
审核摘要：15 个连通分量，孤立实体 14/22（63.64%）；G14 仍待审核。T4 数据刷新经 oo-load 到达 Agent DB，模型调用为零。
实测含探测共 26 次调用、156,828 token（预留 530,874 / 上限 2M）、约 15 分钟；费用未知（provider 未给单价）。有 5 次补全超出预估输出预留，已按 provider 元数据上限执行，3 次不完整尝试经修复重请求补齐；输出预留估计需按 usage 校准收紧。无 Gateway 发布或业务验收声明。证据索引：`tools/pg-builder/tests/selfbuild-thousand-evidence.json`。

## 8. 外部参考（2026-11-03 检索）

- GraphRAG（Leiden 社区 + 层级摘要）：https://arxiv.org/abs/2404.16130
- RIGOR（RAG + 逐步增长的核心本体生成 OWL）：https://arxiv.org/abs/2506.01232v1
- ReFoRCE（超长 schema 表压缩）：https://arxiv.org/abs/2502.00675
- CHESS（分步 schema 选择）：https://arxiv.org/html/2405.16755v1
- LinkAlign（上千字段 schema linking）：https://arxiv.org/html/2503.18596v4
- Spider 2.0（企业级大 schema）：https://openreview.net/forum?id=XmProj9cPs
