# Super Drive（Super Ontology 客户端）· 指针文档草稿

> 发布位置：`puppyone-ai/puppy-issues` 仓库 `document/puppyone-desktop/super-drive/README.md`（D7、20 §8.2 F-Q2）。
> 本文是草稿，放在 OO 仓库内由主线程维护；按 D43 开发期间不对外发布，随 Puppyone 最终 PR 一并提交到 puppy-issues（远端操作需 owner 授权）。

Super Drive 是 Puppyone Desktop 的实验页面（构建 flag `superDrive` 与实验开关 `enableSuperDrive` 均默认关闭）：把文件放进 Drive，由本机 Open Ontology 引擎读懂并建本体，再按任务把一份带出处、按范围隔离的上下文包挂载给 Agent。

规格的唯一来源在 Open Ontology 仓库，不在 Puppyone 仓库内重复：

| 内容 | 位置 |
|---|---|
| 总纲与决策 D1–D79 | `open-ontology@aac3fad1b:specs/super-ontology/00-overview-and-decisions.md` |
| 前端分册（主进程服务、IPC、页面、状态矩阵、i18n、任务卡） | `open-ontology@aac3fad1b:specs/super-ontology/20-frontend.md` |
| 控制 API 契约（Puppyone 侧为 vendored 副本） | `open-ontology:contracts/docs-api/`；Puppyone `contracts/docs-api/SOURCE.json` 记录对应 commit |
| 端到端测试与 owner 验收 | `open-ontology@aac3fad1b:specs/super-ontology/60-e2e-and-owner-acceptance.md` |

数据流说明（D79）：文件和处理结果保存在本机，不上传到 puppyone 云端，除非用户选用 puppyone Cloud。
