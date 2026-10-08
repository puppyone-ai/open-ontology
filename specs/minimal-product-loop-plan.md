# 当前短期目标：接库 → Self-build → Pi 真实问库

2026-10-08，用户明确调整优先级。完整主 Spec/Plan 仍保留，当前里程碑按本文件排优先级，不能宣称整个项目完成。

## 默认路径与依据

依据 docs/09-sandbox-and-agent-runtime.md §6.2：默认 Pi RPC，内置 read/bash 工具读取 /knowledge 文件夹，通过 bash 调 oo CLI，经 Gateway 查询，不依赖 MCP。MCP 属于外部客户端扩展，继续 TODO，不能成为此次 Pi 闭环的前置。Pi 不直接获得数据库连接或模型密钥。

一条命令接入获准本地数据库信息/私有配置 → catalog/profile → 真实 Builder LLM → 标准 OOS YAML/generated 与实际审核状态 → 生成本地 Knowledge Pack 文件夹 → 启动 Pi RPC，先读 README → Agent 自己编写 SQL，用 oo query 调实际 Gateway → 返回有 query_id/真实审计与精确结果的答案 → 关闭并确认会话资源释放。

## 当前交付范围

- 单用户、本地明确 operator development 模式；数据库连接和模型密钥为私有文件引用。企业源资料尚未提供，不连接其测试账号，复用本地私有十万行PG。
- 输入 database type、host/port/db/TLS/SecretRef，检测支持情况并真实连接。首先完成 PG 全闭环；“任意数据库”仅在对应真实连接器通过同闭环后支持，未支持返回明确错误，不能伪称全部兼容。其它 Plan 数据库与文件支持仍保留 TODO。
- Self-build S0–S10质量/来源/模型缓存和失败路径保持原验收；可用结构说明和源声明事实先形成未认证draft Pack，未知和未审核推断如实标注。不能自动将LLM草案标approved、捏造人工审核/Git发布/owner认证。
- 单用户会话可以明确使用本地draft Pack做开发预览，若支持该模式须独立显式配置；它不是active生产发布。正式“YAML→overlay→Git→发布→AgentPack”继续完整主Plan TODO，不阻塞当前明确开发预览，但不能报为已完成。
- Gateway继续真实只读parser/guard、结果精度、预算、超时取消、审计与Secret边界；不得以新的私有SQL模拟server替代现有Gateway实现。认证的多用户管理与角色分配延后，不取消基础只读能力。
- 本地Pi development process须如实标非runsc生产sandbox；production缺runsc仍fail closed。预览模式不伪装发行认证。

## 必须一次完整验收

| ID | 真实输入与行为 | 证据 |
|---|---|---|
| MP-01 | 获准连接信息→实际PGcatalog/profile | 私有连接、100k行数2000/8000/90000、来源/版本hash |
| MP-02 | 真实grok4.7 self-build→标准YAML/Pack目录 | 模型fresh请求/缓存/tokens/cost状态、parse/schema/seal、README/tablefiles |
| MP-03 | Pi先read Pack，再自行bash编写/提交SQL到oo CLI→实际Gateway | 真实Pi RPC工具event、工具SQL、Gatewayquery_id/审计；提示不得提供答案或预写SQL |
| MP-04 | 至少count/关联/条件聚合/null与精确数值真实问答 | 冻结问题独立gold、答案与真实行对应，不只HTTP200或人工proposal |
| MP-05 | 新会话关闭、坏连接/模型失败/SQL写入拒绝 | 无伪成功，无破坏源库；超时取消和资源absence |
| MP-06 | 可复现启动与即时demo | 完整命令/config示例（无密钥），私有fixturecleanstart、复跑复用cache，固定提交/原始退出/全证据SHA |

作者只有MP01–06真实通过才交付；未通过报告明确阻塞，不缩为生成文件或读Pack不查库。Codex验收固定提交，独立必要差异复审，不扩无关admin矩阵。

## 所有权与优先级

P0：a383真实source连接；a373完整selfbuild；a397真实Pi+Gateway；a405一命令本地接库/构建/对话用户流程编排。a399兼容、a389真实100k问题基准为P0支持线。
P1：最小可运行打包、取消/释放与复现证据。前端只在该路径需要时接最小对话，完整admin排后。
P2保留：多用户权限/管理角色API/admin完整页面/发布控制API/Secret管理控制台/外部MCP OAuth/完整运营与发行审批。正在运行的这些任务允许收当前安全固定交付，不启动重复的新功能；没有用户数据的工作不得因优先级降后被删除。

完整主Plan与OC01–06仍待完成；当前验收不改变其完成定义。当前局部79/90不是本里程碑完成率。
