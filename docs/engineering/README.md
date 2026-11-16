# 工程协作与发版规范

> Status: Accepted（据 v0.2 现场加固与 v0.3 快速体验两轮实际运行整理）
> Owner: Open Ontology Engineering
> Last reviewed: 2026-11-16

本目录记录**怎么做事**：协作、测试、CI、发版。**做什么**由 [Spec](../../specs/open-ontology-v0.1-spec.md) 和各版本 Plan 决定，系统怎么设计见 [设计文档](../README.md)。

| 文档 | 内容 |
|---|---|
| [collaboration.md](collaboration.md) | 协调者与执行者分工、任务卡、派发与续跑、签收、台账、提交与合并规则 |
| [testing.md](testing.md) | T0–T10 套件、L0–L3 分层快速测试、缺陷回放闸门、何时跑全量、主机资源约束 |
| [ci.md](ci.md) | PR 门禁清单、托管机环境差异、常见失败与处理办法 |
| [release.md](release.md) | 镜像、离线包、CLI 二进制、npm 包的发版手册与验收 |

与 [产品侧工作形态](../product/01-working-model.md) 的关系：那份文档记录 v0.2 立项时的约定。两份内容冲突时，以本目录为准。
