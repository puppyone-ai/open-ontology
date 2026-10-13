# 原始 Plan / docs 完整数据源需求与验证逻辑核对

核对者：协调者；这是原文需求映射，不是产品测试通过报告。原始内容来源 docs/README、03、04、06、08、12、13、17、database-support/README及03/04，Spec原第18.2 AT、Plan原SP03/SP04/SP07/SP08/SP09/SP13。参考文本均为仓内原设计/验收要求；目标产品事实仍须固定实现与真实证据证明。用户最新完整签收要求保留，未确认解释不能改写原设计。

| 能力 | 原始权威契约 | 验证动作及成功判据 | 不足以签收的证据 |
|---|---|---|---|
| 逐源准入与导入 | docs03 §§3–6；DBX profiles；Plan SP03；AT-SRC01–05/07/10/13/14 | 实际产品/版本/驱动/许可、权限体检、获准表列、真实Arrow到独立PG；预算/所有类型规范值对照；源平台身份零DML/DDL，真实用户取消/恢复 | 连接成功、源COUNT、mock、协议兼容、单一版本推导其他版本 |
| 持续捕获与自定义周期 | docs04 §§4–8/10；docs12 §3；DBX03 §§2–8；Plan SP04-W01–06；AT-PL01–09/SRC08–09 | C1日志CDC验证事务/位点及L0/Lend完整applied屏障；C2–C5按各自token/游标/分区/差分验证。按实际配置调度、更新周期变更、连续新增/更新/物理删除、晚提交/同游标、重放/强杀/过期，停止源写追平后全量逐行零差异；曝光capture/grade/freshness/删除质量警告 | 一次批量导入；seen位置冒applied；测试只改本地PG；把周期任务叫日志CDC；默认自动降级 |
| 可配置周期的语义 | docs12 incremental.every/overlap、overrides.every、limits.window；docs04常驻服务与离散调度区分 | 批量检测/拉取周期与执行窗口按用户配置真实触发，改配置不重叠重复任务；日志CDC常驻流与对账/健康检查周期分别验证 | 把every错误要求套到所有常驻CDC事件；配置字段存在但无调度行为 |
| 完整Self-build | docs06 §4 S0–S10；Plan SP07-W01–06；AT-OOS01–03/05/06 | 真实schema/read_logic/profile；规则关系/码表含证据；实际结构化LLM及缓存/失败；S6–S8候选，S8经审核角色Gateway验证；S9有review决定/操作者；S10PII/semantic_type分类；每stage输入/输出/版本/失败可追溯 | 直接构造Pack，S5一次HTTP200或Pack文件存在；遗漏S9/S10；用count/profile代替Self-build |
| YAML与维护 | docs06 §§3.1/3.12/5/8；Plan SP07-W01、SP08-W02、SP13-W04；AT-OOS01/06/09 | ontology.yaml及generated/curated/reviews严格OOS校验；effective按稳定ID合并、curated不覆盖、rejected移除；事实变更stale待审；table_ready/schema_changed/定时/手动/feedback触发；未变输入命中缓存。新增敏感列隐藏、改类型暂停、未知码值未确认、反馈只进审核 | Markdown/JSON充当YAML真相；LLM或Agent直接改curated/自动扩大权限；每条数据更新强制发布新Ontology |
| 审核发布及实际Pack | docs06 §§6–8；Plan SP08-W04；AT-OOS04/05/11 | 按适用G1–G13原门槛静态/真实catalog一致、分类、独立control_total、owner变更审阅、角色ACL/RLS/副本屏障/CAS和失败封闭；Git版本及不可变产物；同角色API/挂载manifest一致、目录预算、旧权限收缩失效 | 未审核草案称已发布；旁路sidecar flags；人工改sealed catalog；unit字符串断言称真实PackCLI成功 |
| 数据与语义持续一致 | docs06 §8、docs04质量/结构变化、Spec DEC04/05 | 会话固定语义版本但当前授权；动态数据按runtime freshness和质量；结构变化与YAML/catalog类型对照，先隐藏/暂停/审核发布再开放；授权收缩立即生效 | 要求每次CDC行变动都重建Ontology；把运行freshness固化在YAML；旧Pack授权永不变化 |
| 外部Agent真实消费 | docs08 §§1–7；Plan SP09/13；AT-API01–03/MCP01–03/GW03–06 | 非Pi实际harness通过已实现CLI/HTTP/SDK/MCP读取同角色Knowledge、describe、query及freshness；真实多轮模型自主工具调用，答案对独立预期、query_id审计匹配、精确decimal、警告/拒绝/取消/撤权；不接触源密码/直接DB，入口按客户端/协议/version实际验 | 人工脚本代Agent、多条固定SQL称真实对话；已有SDK示例称已实现MCP；任何harness理解为所有现有客户端已测 |
| Ontology符合真实表/质量 | docs06 §9、docs13轨道D/E；Plan SP07/SP12；AT-OOS02/03/07–10 | 表列/类型与真实catalog匹配；语义类型/关系含基数/码表/描述对独立真值及领域owner；PII召回100%，实体自动合并精确率目标≥99.9%；公开/客户及C0–C4消融依原适用门槛分开验证 | 只验YAML语法或行数；只靠模型自评正确；无业务owner仍certify指标 |

## 十源捕获区别（原DBX逐库设计，并非已实现事实）

PG/MySQL/MariaDB有C1日志路线，SQL Server有C1或C2，MongoDB C1须副本集/分片（standalone不能声明CDC）。Oracle先批量S1，LogMiner等C1为单独能力和ADR/许可/零写门槛，不自动启用R1w。ClickHouse/Doris/StarRocks使用C4/C3/C5，OpenSearch C5或C3，原文不宣称日志CDC。C3不原生感知删除，须对账并DELETES_LAGGED；C4分区外删除能力不能外推。capture不同不取消“持续更新/可配置周期/真实删除收敛”的需求。

人类已明确选择：按原 docs 逐源方法，验证可配置周期与更新、删除收敛。日志CDC、原生变更读取、游标增量、分区刷新、全量差分按C1–C5各自实际能力验收，不能混称或静默降级。

## 对上一轮目标/台账改动的复核

1. “历史受控preview6/10不是完整签收”成立，完整签收不能由缺失CDC、OOS审核/维护及外部对话的局部证据推导；保持0/10待证，不抹除六源preview固定成功。
2. “所有源统一日志CDC”和“所有数据变更重建同一个Ontology版本”不是原docs要求。5d50211ca中的广义CDC口径已由人类澄清为原文逐源C1–C5持续捕获，不能派成不存在的源能力。
3. 原设计明确generated→curated/effective→分类→G门禁→不可变Pack/API，上一轮概括缺少逐stage/审核/发布/权限和版本验证映射；本表补齐逻辑，尚不宣称执行通过。
4. 任何Agent属于产物/协议开放设计；验证按实际客户端/接口/version声明。OO CLI可用不代MCP完成，MCP也不能代所有harness完成；原SP13全部必交仍保留。
5. 数据准入、组件作者交付、preview、完整源/父项、整版、正式grade/nightly/发行分层；原任务和失败记录复用，不增加返工功能分母。

## 当前行动边界

本轮先记录需求审计，依据上述原文审计及人类明确选择同步纠正Goal/台账的逐源捕获表述；DL顺序不变。05/41既有唯一Grok工程师完成封存语义/doc保真原子公共能力仍有效，不是完整源签收。公共原子能力fixed后联结原全部要求，无需让每个源重复开发公共Self-build/Loader/Gateway。完整源工程师负责其真正逐源旅程和验收证据；缺外部凭据/DBA实例/审批就保留原门槛及精确blocked条件，不用假成功替代。
