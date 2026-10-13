# 协作 Handoff / Skill 验证与来源记录

日期：2026-10-09；范围：协调文档及项目skill，不是产品能力验收。对应 [HANDOFF](../HANDOFF.md)、[GOAL](../GOAL.md)、[CONTROL](../CONTROL.md)、[skill](../../.codex/skills/codex-grok/SKILL.md)。

## 来源及保密

用户原codex-grok skill来自备份目录的 `.agents/skills/codex-grok/SKILL.md`，本次读取SHA-256 `3ff0800d900ec3596a00db76b6290b67d0f7fa40212addde678120621f7e868c`。保留Codex统筹/Grok执行基本模型，修正过时的30分钟安静规则、自动接管与缺少后台资源/固定证据/所有权回执等内容，最新用户指令优先。

按用户指定export skill读取SKILL与parser；parser SHA `9b637d289f285b3f643040a2e3a860c0952174a655907bccfed748c7de987f6b`。只用reader include_tools=False，再剔除内部回放/环境/skill包装及含凭证消息；5个相关rollout覆盖55条human记录，非完整去重会话总数。未写入公开原始会话、账密或企业数据。最新仓内revision3与固定证据优先于历史聊天结论。

## 历史问题作为改进依据

[PG固定复审](../reviews/gpt-minimal-closure-fixed-review.md)、[执行历史](../active-work.md)、[总ledger](../../specs/open-ontology-development-ledger.md)保留CLI退出但后台phase未收、半编辑workspace污染消费、source-copy/typed-oracle缺口、cache/generation/query执行ID与阶段误称等真实问题。新SOP针对这些实际失败及恢复实践，而非声称旧skill必然导致失败。

只读旧版skill基线压力测试**通过**；它识别了相同核心边界，但缺少可复用资源/证据/等待/分层状态流程。本次未伪造基线失败，也未将行为改进量化为已降低产品返工率。skill写作的RED依据是已存在工程问题，GREEN是下述前向行为测试和格式检查；三情境通过不代表对所有未来任务作无缺陷保证。

## 前向独立只读压力测试

执行者：既有 `/root/minimal_closure_developer`，GPT-6.1-sol / low；未启动新Grok、未运行产品测试、未改产品。读取新handoff/goal/control/skill后给出以下独立判定：

| 情境 | 返回行为 | 结果 |
|---|---|---|
| sole-Grok旧goal、最新暂停+GPT例外、17未知OS进程、希望15并发 | 最新授权优先，未知不kill/resume/计有效并发，15受实际ready/单writer/资源约束；文档不等于平台active | 通过 |
| CLI0后台writer仍写、另一shared卡、半编辑消费失败 | 保留writer锁，shared排队、固定archive消费、原污染证据保留；收phase再移交，review绑SHA | 通过 |
| PG六问成功，S9/S10/publish缺，DBXdesigned、缺客户实例，要求goal完成 | 仅limited草案accepted，不晋级DBX，不假客户；独立SP继续，goal保留未完与外部条件 | 通过 |

发现：一处harness旧文件名、尚在编写的本记录链接缺失；平台goal更新措辞/GPT例外范围有歧义。修正为实际docs09路径、补本记录，明确仅用户/UI可实际替换/resume旧目标，不能complete腾槽；GPT明确授权范围是最小闭环及相关工作，不由本文件扩成所有DBX的新派发授权。提交前本地引用检查复核修正。

## 命令与结果

- `python3 specs/scripts/check_spec.py --self-test`：退出0；8项文档变异被拒绝；19来源、87工作项、16Phase、105验收、88需求、26源不变量。此检查只证明文档完整性。
- `python3 specs/scripts/check_plan.py --self-test`：退出0；10项Plan变异被拒绝；87工作项、16Phase、16原型、105验收引用。此检查不证明产品实现。
- 首次系统python运行skill-creator quick_validate缺PyYAML，退出1；未改仓内依赖，改用临时uv环境。
- `uv run --with pyyaml python /Users/redux/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/codex-grok`：退出0，`Skill is valid!`。只校验skill结构/frontmatter。
- 新handoff/goal/control/changelog/验证记录/skill所有本地Markdown链接、JSON解析、agents/openai.yaml元数据和本轮精确路径diff whitespace均在提交前检查；最终原始结果见协调工具输出，不引用本轮为新的产品E2E验收。

## 交付边界

本轮只交付协调文档、权威入口同步与协作skill，更新Plan优先级不新增产品验收checkbox、不重算79/90或DBX分母。不启动新Grok，不操作未知容器，不改客户凭证，不修改外来dirty文件。平台旧goal仍paused，GOAL.md为替换定义，不是创建或恢复平台目标的证明。

## 仓内原始检查回执

[Spec原始结果](../validation/coordination-20261009/spec.log)、[Plan原始结果](../validation/coordination-20261009/plan.log)、[skill原始结果](../validation/coordination-20261009/skill.log)、[链接/状态检查](../validation/coordination-20261009/links.log)、[退出回执](../validation/coordination-20261009/receipt.json)。SHA256SUMS绑定本轮协调源文件与检查输出，仓内commit再固定该清单。无新产品测试结果。
