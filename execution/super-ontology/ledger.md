# Super Ontology 台账

按 50 §9 维护：每张卡一行，每个闭环签收追加一行。状态只允许 todo、in_progress、review、merged、signed、dropped。主线程独占本文件。

| 卡号 | 闭环 | 状态 | 分支 | 提交（首 → 末） | PR | 证据 | 备注 |
|---|---|---|---|---|---|---|---|
| SO-SPEC | — | merged | feat/super-ontology (主线程) | 台账、spec 类提交 | — | — | 非卡片的主线程提交 |
| SO-B-S1-01 | L1 | merged | feat/super-ontology (主线程) | 5f1fb464e → 5f1fb464e | — | contracts/docs-api/ | docs-api 1.0-draft |
| SO-B-S1-02 | L1 | merged | feat/super-ontology (主线程) | b5b6d3831 → b5b6d3831 | — | — | oo-docs 骨架；arch 新增 FEATURE_GATED_EDGES |
| SO-B-S1-03 | L1 | merged | feat/super-ontology (主线程) | 9cedcc05c → e0c5f080f | — | — | docir、DocumentSource、context-pack schema |
| SO-B-S1-04 | L1 | merged | so/B-S1-04-golden-set | b44b1736c → 07834ceab | — | — | 51 份文档 + 4 红队 |
| SO-B-S1-05 | L1 | merged | so/B-S1-05-oo-eval-docs | ec491bdf7 → ec491bdf7 | — | — | 自带 JSON 解析器，L2 前改用 serde_json |
| SO-B-S1-06 | L1 | merged | so/B-S1-06-trailer-ledger-checks | edde0add0 → edde0add0 | — | — | — |
| SO-B-S1-07 | L1 | in_progress | so/B-S1-07-supply-chain | — | — | — | 子智能体起草补丁 |
| SO-B-S1-08 | L1 | merged | so/B-S1-08-private-terms | 462d12cf3 → 462d12cf3 | — | — | CI 需加密变量注入词表（owner） |
| SO-V-S1-01 | L1 | in_progress | so/V-S1-01-formal-toolchain | — | — | — | quint、kani 已装；待续作 |
| SO-V-S1-02 | L1 | todo | — | — | — | — | FM1 |
| SO-F-S1-01 | L1 | in_progress | codex/super-drive-F-S1-01-* | — | — | — | Puppyone |
| SO-F-S1-02 | L1 | in_progress | codex/super-drive-F-S1-02-* | — | — | — | Puppyone |
| SO-F-S1-03 | L1 | in_progress | codex/super-drive-F-S1-03-* | — | — | — | Puppyone |
| SO-F-S1-04 | L1 | in_progress | codex/super-drive-F-S1-04-* | — | — | — | Puppyone |
| SO-F-S1-05 | L1 | todo | — | — | — | — | 依赖 B-S1-01（5f1fb464e） |
| SO-F-S1-06 | L1 | todo | — | — | — | — | — |
| SO-F-S1-07 | L1 | in_progress | — | — | — | — | 指针文档 |
| SO-B-S2-01 | L2 | todo | — | — | — | — | — |
| SO-B-S2-02 | L2 | todo | — | — | — | — | — |
| SO-B-S2-03 | L2 | todo | — | — | — | — | — |
| SO-B-S2-04 | L2 | todo | — | — | — | — | — |
| SO-B-S2-05 | L2 | todo | — | — | — | — | — |
| SO-B-S2-06 | L2 | todo | — | — | — | — | — |
| SO-B-S2-07 | L2 | todo | — | — | — | — | — |
| SO-B-S2-08 | L2 | todo | — | — | — | — | — |
| SO-B-S2-09 | L2 | todo | — | — | — | — | — |
| SO-B-S2-10 | L2 | todo | — | — | — | — | — |
| SO-B-S2-11 | L2 | todo | — | — | — | — | — |
| SO-B-S2-12 | L2 | todo | — | — | — | — | — |
| SO-F-S2-01 | L2 | todo | — | — | — | — | — |
| SO-F-S2-02 | L2 | todo | — | — | — | — | — |
| SO-F-S2-03 | L2 | todo | — | — | — | — | — |
| SO-F-S2-04 | L2 | todo | — | — | — | — | — |
| SO-F-S2-05 | L2 | todo | — | — | — | — | — |
| SO-F-S2-06 | L2 | todo | — | — | — | — | — |
| SO-F-S2-07 | L2 | todo | — | — | — | — | — |
| SO-F-S2-08 | L2 | todo | — | — | — | — | — |
| SO-F-S2-09 | L2 | todo | — | — | — | — | — |
| SO-F-S2-10 | L2 | todo | — | — | — | — | — |
| SO-F-S2-11 | L2 | todo | — | — | — | — | — |
| SO-F-S2-12 | L2 | todo | — | — | — | — | — |
| SO-F-S2-13 | L2 | todo | — | — | — | — | — |
| SO-V-S2-01 | L2 | todo | — | — | — | — | — |
| SO-V-S2-02 | L2 | todo | — | — | — | — | — |
| SO-V-S2-03 | L2 | todo | — | — | — | — | — |

## 签收

| 闭环 | 签收提交 | OO sha | Puppyone sha | 证据 | 日期 |
|---|---|---|---|---|---|
