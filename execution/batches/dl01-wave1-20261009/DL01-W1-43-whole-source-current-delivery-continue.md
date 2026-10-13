# DL01-W1-43 同一完整源长跑继续

必须先完整阅读原任务卡 `execution/batches/dl01-wave1-20261009/DL01-W1-43-full-maintained-ontology-source-longrun.md`、execution/INDEX.md、GOAL.md、PRODUCT-LOOPS.md、CONTROL.md、TODO.md、UPDATE-PROTOCOL.md、.codex/skills/codex-grok/SKILL.md与该源原docs/Spec/Plan。原所有输入/接口/独占路径/量化退出保持。

继续原Doris完整任务：现场14native代保持、R0sealer由02/06修复；与74协调真实测试DBA独立身份的新增/更新/删除与可配置周期、完整目标水位/lease/fence/source-bound/逐格验证闭环，未明确互斥窗口不得改74正在读表。完成本源真实Builder输入交接至71，不造sealed/cards；源43旧FE/BE不重启，新ns2保留。


## 新收到固定跨源返工（2026-10-12，本原完整任务继续）
先读 execution/reviews/dl01-wave1-native-loader-fixed-review/HANDOFF.md 与 FINDINGS.md。独立复审65abb6c7f9ee8af20ecd50de5a6a2e66f15750b0 REQUEST_CHANGES：自写receipt得trusted、客户端grade、expired position被丢弃、truth改变被replay、空lease仍封存；全部按原输入信任与真实lease/fence修复，不弱化条件。Oracle4fa46c07fe59fb1fda05e41130da9b64ee767978真COPY列名二次mapping失败/int16key拒绝；Mongo c41d31698fef43a452c93c766e5682d4a24d7054 native两代0cells但document schema.invalid/profile缺真实sealed输入；Star6026b349105f9cde711d9a8eccdff4a9aa8a019b 20次native0cells但registry与receipt互含hash导致digest_mismatch。按独占接口对接02/71/74，复用固定真实输入，完整交付与失败无副作用，不以source_bound作者布尔冒充全源。

原CLI已终态，具名原背景构建亦终态；先收取现存资源再开新计算。只Grok，无嵌套agent，无CPU/jobs/编译槽限制；不访问其他凭据。approved llm-token仍空目录，S5真实模型与Agent多轮缺条件保持pending；不把工程接口缺失归类外部门槛。未收到新固定接口先完成独占范围可执行工程与交接，不能复跑无变化成功项来凑并行。完成REPORT/FINDINGS/HANDOFF、固定SHA与raw/hash、实际资源回执；整源全退出未满足保持not_claimed。
