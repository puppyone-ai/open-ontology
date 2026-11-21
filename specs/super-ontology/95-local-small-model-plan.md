# 95 · 本地小模型（Qwen3.5-9B MLX）构建本体：方案与执行 lane

> 来源：owner 2026-11-21 要求；方案由“Super ontology 产品形态规划”会话整理，执行会话落库。两路调研子智能体本次都没有联网逐条核对，下面的工具 API、版本、速度数字都按“未核”处理，每个 lane 第一步先实测确认；所有提升都是推断，以 L4 golden/holdout 和验收语料上的实测为准，对照当前默认 cb/gpt-6.2-sol（golden 事实 .911/.795、holdout .912/.759，published view）。

## 1. 总原则
小模型不靠“更聪明”，靠三件事补：把智力搬进程序（预处理、规则、实体解析、约束）、把任务拆成窄而可验证的小步、用同一模型多采样加确定性校验。调研结论：异构小模型 MoA、多智能体辩论证据弱（Self-MoA arXiv 2502.00674 显示同一强模型多采样优于混合弱模型），不做主线；投机解码只提速不提质；模型合并只省内存。

## 2. owner 点名的三个弊端与对策
1) 上下文有限 → 结构化切块：按标题/段落/发言轮次/表格行切，600–1200 中文字、带少量重叠，块 id 稳定；每块附“文档头”（标题、日期、参会人、所属项目），解决代词与“下周”这类相对时间；提示只带本块 + ≤50 个已知实体名 + 2–4 个短示例；schema 分批（每次 ≤8 类型，学 OneKE）。
2) 智力有限 → 预处理与程序化：表格/xlsx/csv、frontmatter、目录约定继续走现有规则（rules 早发布已在）；正则抽日期、金额、邮箱、工号并归一化为绝对值；HanLP（Apache-2.0）+ 已有别名词典生成实体候选；模型任务从开放生成改为“候选判别 / 多选题”（这对实体是不是 X 类型、这对实体是什么关系含“无”）；实体合并改为 Splink（Fellegi-Sunter，MIT）打分，只有灰区对交给模型做二元“是否同指”；时间与取代、冲突继续由程序判定（双时间、D57 约束），模型只回答“原文是否明确说 B 取代 A”并给 quote。
3) 输出格式不稳 → 约束解码 + 解析器：mlx-lm 的 logits_processors 挂 XGrammar 或 llguidance（MIT），或 Outlines 的 MLX 后端，JSON Schema 约束；字段顺序 quote → 实体 → 类型（先引原文再下结论）；可选把 quote 约束为本块子串（后缀自动机/trie），从根上消除伪造引文；容错解析器作为第二道：剥 markdown 围栏、截断修复、单引号/尾逗号修复、按 schema 宽松强转，失败带错误信息重试一次，再用 rapidfuzz 对齐原文，再丢弃并计数。

## 3. 小模型拼出大模型能力（重点）
推荐架构，按证据强弱：
- 同一模型多采样 + 确定性投票：关系与时间字段 k=3–5（temperature ~0.7），元组（主体、谓语、客体、时间）≥⌈k/2⌉ 票才收；一致率作为置信度（替代模型自报与 logprob）。依据 self-consistency 2203.11171、Snell 2408.03314、Large Language Monkeys 2407.21787：前提是有验证器，而我们有天然验证器（quote 子串、schema、domain/range、约束）。
- 不确定才升级（本地级联）：触发 = 票数分歧、quote 校验失败、schema 失败；升级顺序 = 开 thinking + k 提到 8–16 → 若内存允许，更大的本地模型 → 标待确认，绝不静默写入。云端复核只作可选开关，默认关（符合 D63 不中转、数据在本地）。
- 分任务 LoRA 蒸馏：用当前默认云端模型在我们自己的合成语料上产标注 3–5k 条，mlx-lm lora/QLoRA 训 NER、关系两个适配器；训练格式借鉴 OneKE 的 schema 指令与负采样（给原文不存在的类型要求输出空）。不使用 UniversalNER、IEPile 等 NC 许可数据与权重；Qwen3.5 权重许可、云端模型条款是否允许用输出训练，执行前核实并记入 supply-chain。
- 生成与验证分离：模型判别只做窄二元题；验证尽量用程序。
- 不做：异构 MoA、辩论、GenFuser 式自由融合、模型合并（除非为省内存）。

## 4. 推理工程
mlx-lm server 或 batch_generate 做批处理；文档头 + schema + 示例前缀走 prompt cache；量化对比 bf16 / 8bit / 4bit 的 F1；可选 Qwen 0.6B 草稿模型投机解码（先确认与约束解码兼容）；thinking 默认关，只在消歧、时间、取代判定步开，A/B 定。沿用 OO_DOCS_MODEL_TIMEOUT_SECS、RPM、并发配置，新增 provider=mlx-local（OpenAI 兼容 egress 指向本机 loopback 端口），数据不出本机。

## 5. 执行 lanes（每个 lane 产出 testing/super-ontology/evidence/L4/eval/local-model/<lane>/，与默认模型同口径对照）
- LM0 基线拆解：mlx 本地服务 Qwen3.5-9B 接入 egress（provider=mlx-local），零样本 extract/4 原样跑 golden/holdout；按错误类型统计（格式错、漏抽、错抽、时间错、消歧错、quote 伪造），同时实测 tok/s、单文件延迟、峰值内存、bf16 vs 8bit vs 4bit。这是后面所有 lane 的对照。
- LM1 格式：约束解码（XGrammar/llguidance/Outlines 任选实测最稳的）+ 容错解析器 + quote 前置字段；目标格式失败率≈0，测对 F1 的影响（含“约束损害推理”检验）。
- LM2 切块与预处理：结构化切块 + 文档头 + 正则/HanLP 候选 + schema 分批；对照 LM1。
- LM3 分步判别流水线：NER（候选判别 + 开放补充）→ 关系多选题 → 时间 → 跨块消解（Splink + 灰区二元判别）；对照 LM2。
- LM4 多采样投票与本地级联：k=1/3/5/9 的 F1-算力曲线，投票一致率作置信度，不确定升级规则；对照 LM3。
- LM5 LoRA 蒸馏（依赖 LM0–LM4 定下的提示与格式）：标注 3–5k、训 NER 与关系适配器、热切换；对照 LM4。语料留出：蒸馏只用训练用语料，golden/holdout/acceptance 不得进入训练集。
- LM6 吞吐：批处理、prompt cache、量化、可选投机解码；目标是验收语料全量冷启动时间不劣于当前云端方案，并报告 45 文件验收语料与 1000 文件规模的耗时。

建议顺序：LM0 先行；LM1、LM2、LM6 可并行；LM3 依赖 LM1+LM2；LM4 依赖 LM3；LM5 最后。

## 6. 决策点与退出条件（交给 owner 拍板，不提前改默认）
- 每个 lane 报：golden/holdout P/R/F1（published view）、acceptance relations 与 Ask、格式失败率、quote 伪造率、延迟与内存，与默认模型并排。
- 只有当本地方案在 holdout 上 F1 与默认差距 ≤ 0.03（或 owner 另定），且 acceptance Ask 不低于 29/30 时，才提议把默认切到本地；否则本地作为可选 provider 保留，并写明差距所在的子任务。
- 预期（推断）：零样本约为云端的 0.6–0.7；加预处理、分步、约束、投票约 0.80/0.65；再加 LoRA 有望接近 0.88/0.75。实测与此不符就按实测报告，不调阈值。
- 不改：评测集与金标、红队与可见性门禁、商用边界（90 分册）、数据在本地（D79）。
