# Super Ontology 本地小模型方案（延后）

> 状态：**延后（owner 2026-11-21 决定本期不做）**。本文为联网核实后的修订版，未经 owner 批准生效前：不启动任何 LM lane，不改 egress/provider 配置，不影响当前默认模型。
> 核实方式：2026-11-21 联网打开一手来源（GitHub 仓库/issue/release、HF 模型卡、arXiv、官方文档），并检索 HN、GitHub Discussions 等社区；reddit.com、部分 X 与知乎页面工具无法访问，相关内容标“未核”。所有提升均为推断，以 L4 golden/holdout 与验收语料实测为准，对照当前默认 cb/gpt-6.2-sol（golden .911/.795、holdout .912/.759，published view）。

## 1. 结论
1. 社区没有任何 9B 级模型在 KG/IE 抽取上达到 F1 0.91 的实证；证据只支持“蒸馏后明显提升”。目标是先测出真实差距，再决定本地模型做默认还是可选。
2. Qwen3.5-9B（2026-03 发布，Apache-2.0，32 层中 24 层 Gated DeltaNet，原生 262K 上下文）在 MLX 上的已知硬伤（均为 open issue）：
   - LoRA 训练：GatedDeltaNet 无 Metal 反向 kernel，退回逐 token Python 循环（mlx-lm #1956）；序列 >512 时 OOM（mlx #3539）；mlx-vlm 训出的 adapter 损坏（mlx-vlm #824）。
   - 投机解码：外接草稿模型在 Qwen3.5 上导致 server 崩溃或丢 token（mlx-lm #1446、#846、#1423）；开草稿模型时 server 关闭批处理。
   - thinking：官方 chat_template 默认开启，不支持 `/no_think` 软开关；每个请求必须显式传 `enable_thinking=false`；non-thinking 采样 0.7/0.8/20/0 + presence_penalty 1.5；有推理死循环报告。
3. 结构化输出：`mlx_lm.server` 无 `response_format`；Ollama MLX 后端静默忽略 schema（ollama #17933）；Outlines MLX 后端（`outlines.from_mlxlm`，1.3.3）不支持与批处理同时用。可行路径：自写 Python harness，mlx-lm `BatchGenerator`（0.32.x），每条序列挂一个 llguidance matcher（MIT，`llguidance.mlx` 自带 Metal kernel）；同一进程内不混跑带约束与不带约束请求（vllm-mlx #784 交叉污染）。
4. 社区教训一致：Graphiti、GraphRAG、LightRAG、Cognee 用小模型主要败在 JSON 格式；LightRAG 现行 README 建议本地最低 Qwen3-30B-A3B、抽取用 non-thinking。约束解码对抽取利大于弊（dottxt 对 “Let Me Speak Freely?” 的复现、JSONSchemaBench），推理字段放在答案前或单独一步。
5. 多个小模型拼接：有证据的是同一模型多采样 + 投票（只作置信度信号，高一致可能是集体幻觉，arXiv 2604.11104）、低置信级联升级（FrugalGPT、RouteLLM、AutoMix）、专门训练的小验证器（MiniCheck 2404.10774）、程序化实体消歧（Splink，MIT）。不做：异构 MoA（IE 无正面证据）、多智能体辩论（算力对齐后多数持平或落后，2310.01798、2502.08788）、模型合并（mergekit 为 LGPL-3.0，IE 无证据）、投机解码（见上）。LoRAX（已归档、仅 CUDA）与 S-LoRA（CUDA）在本机不可用。
6. 商用许可：UniNER（CC BY-NC）、OneKE 与 IEPile（CC BY-NC-SA）、InstructIE、DuIE 均不可用于商用训练，只可评测；蒸馏数据只用自有合成语料，由云端模型标注且只保留 quote 校验通过的样本；须先核实云端模型条款是否允许用其输出训练。

## 2. 三个弊端与对策
| 弊端 | 对策 |
|---|---|
| 上下文有限 | 按文档结构切块（≤约 1000 字，稳定块 id）+ 文档头（标题、日期、参会人、项目）；可先切原子事实（iText2KG）；每次只问 4–6 个类型（OneKE schema 分批） |
| 智力有限 | 规则处理表格、frontmatter、日期、金额、邮箱；HanLP（Apache-2.0）+ 别名词典生成候选；关系端点只能是已抽实体 id；时间事实单独一步；补抽 1 轮；Splink 消歧、只把灰区交模型做选择题；冲突与取代由程序判定，模型只回答“原文是否明确说 B 取代 A” |
| 格式不稳 | llguidance JSON Schema 约束、quote 字段最前；容错解析器；quote 精确匹配 + rapidfuzz 模糊对齐（参照 LangExtract），对不上丢弃；死循环检测与 max_tokens 硬上限 |

## 3. 执行 lane（批准后才执行）
| lane | 内容 | 产出 |
|---|---|---|
| LM0 基线 | 自写 harness（BatchGenerator + llguidance），`mlx-community/Qwen3.5-9B-8bit` 显式关 thinking，零样本跑 golden、holdout、验收语料；对照 Qwen3-14B（纯 attention，LoRA 与投机解码可用），内存允许时再对照 Qwen3.6-35B-A3B | 错误类型分布、速度、内存、4bit/8bit/bf16 对比；决定主力模型 |
| LM1 格式 | 约束解码、容错解析、quote 对齐、死循环防护 | 格式失败率≈0 及对 F1 的影响 |
| LM2 预处理与分步 | 结构化切块、文档头、规则与 HanLP 候选、实体→关系→时间三步、补抽、schema 分批 | 对照 LM1 |
| LM3 消歧与验证 | Splink + 灰区选择题；小验证器（先零样本 4B，蒸馏后替换） | 对照 LM2 |
| LM4 采样与级联 | k=1/3/5 投票曲线（只作置信度）；低置信先开 thinking 重试、再升级更大本地模型、仍不定则标待确认 | F1–算力曲线 |
| LM5 蒸馏 | 自有语料、云端标注 ≥3k、只留 quote 通过样本；主力若为 Qwen3.5，等 mlx-lm #1956 修复或在 GPU 上用 PEFT 训练后转 MLX | 对照 LM4 |
| LM6 吞吐 | 批处理、前缀缓存、量化档位；不开投机解码 | 验收语料全量耗时、1000 文件规模耗时 |

顺序：LM0 → 并行 LM1、LM2、LM6 → LM3 → LM4 → LM5。golden、holdout、验收语料不得进入训练集。

## 4. 启用前须 owner 决定
1. 批准本方案生效。
2. LM0 是否同时测 Qwen3-14B 与 Qwen3.6-35B-A3B（Qwen3.5 训练问题未修时可能换主力）。
3. 低置信条目是否允许送回云端复核（默认关；打开则片段离开本机，D79 的“数据在本地”说明须随之变化）。
4. 默认切换门槛：建议 holdout F1 与默认差距 ≤0.03 且 Ask ≥29/30，否则作为可选 provider 保留。
5. 用云端模型输出蒸馏的服务条款核实（owner 或法务）。

## 5. 不改
评测集与金标、红队与可见性门禁、90 分册商用边界、D79 数据在本地。
