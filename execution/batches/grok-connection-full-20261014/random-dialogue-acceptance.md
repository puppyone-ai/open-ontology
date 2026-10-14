# 真实随机 CLI + Agent 对话：十源新增验收条件

用户2026-10-14直接要求：每次实际问答必须变化，CLI支持随机问题；这是原十源完整链的测试加强，不新增功能分母、不降低原退出。固定负例/单元测试仍可复现；历史固定问答保留，不能代替新增真实随机出口。

输入：每源当前已授权角色可见真实表/字段/数据、当前 source generation / Ontology / Pack、实际 OOCLI/Gateway、源侧独立只读真值。不得从待测Gateway输出生成gold，不给Agent源账号、gold、预写SQL或预写tool调用。

最小实现：复用 tools/agent-product-verifier/dialogue.py 的现有自主工具循环及事实/QID/WAL核验，在薄测试编排层随机选择问题，无需另建产品框架/新依赖/重写Rust CLI。产品查询/权限/类型核心保持Rust。源作者准备有真实语义差异的候选问题（表、字段、过滤值、聚合或明细目标的变化，而非只换措辞），对应源侧独立预采集gold。题库至少四个不同语义问题、覆盖两类查询；两次各两轮不得靠同一组题换顺序过关。语义标识须绑定真实表/字段/过滤或任务目标，不能为同一题换随机ID。

每次正式新运行使用新随机seed；在首次模型调用前冻结实际选择的问题、语义标识、seed、独立gold hash、源/Pack/角色身份。新运行所选语义问题不得重复上一新运行中的题；仅重排旧题或随机措辞拒绝或重新选取。使用上一冻结manifest作反重复证据；需要更多新题时从真实源条件扩展并重新冻结独立gold。题池耗尽显式报不足，不能把旧固定记录标random。支持指定seed进行故障回放，标replay，不计新的随机运行；resume不得重新抽题，原问题与真值摘要必须匹配。校验非法/重复语义ID、缺gold、缺题库和resume错配，均在任何模型调用前拒绝。

完整每源出口：两次真实新随机运行，每次至少两轮，由模型自主选工具形成不同的语义问题序列；每轮实际OOCLI查询、成功QID独立WAL、答案逐事实匹配冻结源侧gold，当前Pack/权限/数据代一致。模型可选择不同合法SQL及工具步骤，不能断言固定SQL字符串。必要tables/describe/knowledge/freshness覆盖沿用原规则。每轮CLI适配实际任意合法查询参数，禁止仅支持预设题/SQL。旧链已证不重放S5/import/DML；只新增必要真实对话调用，保留真实模型身份/usage/失败。

证据：固定代码SHA、CLI/Gateway二进制SHA、源代/角色/Pack SHA、题库与selected manifest/gold hash、两次seed/语义差异、raw transcript/QID/WAL/typed verdict及资源退出。只测试抽样函数不等于真实随机对话通过；未执行源明确pending，不勾完成。

唯一共享验收器writer：原ClickHouse工程师26ae6875-cfaf-457a-b566-ab548bdc6989，新增仅tools/agent-product-verifier/dialogue.py、tests/test_dialogue.py与CONTRACT.md写域；其他源只读固定提交，私有自有fixture题库/配置自行准备。共享入口owner仍只负责原目录/compiler，不修改此验收器。ClickHouse必须同时完成自己的实际随机全链示范和原完整源退出，非组件短交接。
