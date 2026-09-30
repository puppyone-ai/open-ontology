# 15 术语表

> Status: Proposed
> Last reviewed: 2026-09-30

| 术语 | 含义 |
|---|---|
| **Open Ontology（OO）** | 本项目。把企业数据库变成面向 Agent 的数据层 |
| **Ontology** | 本项目中指面向 Agent 的数据库语义与访问层（数据面 + 语义面 + 访问面），**不是知识图谱** |
| **Ontology Spec（OOS）** | Ontology 语义面的结构化规格（YAML），Git 中的真相源，见 [06](06-ontology-layer.md) |
| **generated / curated** | OOS 的两层：Builder 自动产出的层与人工编辑的层；编译时 curated 优先 |
| **Knowledge Pack** | OOS 按角色编译出的只读产物（Markdown + JSON），挂载在沙盒 `/knowledge`，也通过 API 提供 |
| **Ontology Builder** | 自动构建 OOS 草稿的组件（画像、关系推断、码表识别、LLM 起草等） |
| **Dataset** | OOS 中的一张表或视图 |
| **Entity** | 业务实体（客户、运单），映射到一个主 Dataset 与若干相关 Dataset |
| **Relationship** | 两个 Dataset 之间的 JOIN 路径，带基数与证据 |
| **Enum（码表）** | 状态码等编码值的释义 |
| **Metric（指标/口径）** | 带规范 SQL、时间归属、单位、负责人的统计定义 |
| **Verified Query** | 经人确认的“问题 + SQL”示例 |
| **tier** | Dataset 层级：`raw`（源表副本）、`scene`（场景表）、`file`（文件来源） |
| **场景 schema** | 由 `after_load` hook 在 Agent DB 中构建的业务化表/视图 |
| **源库** | 企业现有的业务数据库，平台只读 |
| **Agent DB** | 独立的 PostgreSQL，存放同步来的数据，只服务 Agent |
| **控制库** | 存放运行状态、任务、审计索引、评测结果的 PostgreSQL，与 Agent DB 分开 |
| **`_oo` schema** | Agent 不可读的派生 schema：新鲜度、源水位、质量状态和 RLS 辅助函数 |
| **`oo_dict` / `oo_entity`** | 按角色授权的派生 schema：码表/业务日历与规范实体/成员/别名，可从 Ontology 和控制库重建 |
| **transactional / per_table** | CDC 默认按源事务应用 / 显式逐表降级；分段或暂停期间必须标记跨表不一致 |
| **源水位** | 最后完整应用的源位点；不一致期间不能据此声称当前全部可见数据处于一致切面 |
| **拒绝记录（rejects）** | 转换失败或非法值替换的逐值记录；保留原行，敏感值掩码，汇总进入查询质量警告 |
| **质量警告（warnings）** | 随查询与审计返回的数据质量/一致性/新鲜度影响，Agent 必须在答案中披露 |
| **有效期 / 别名等级** | 语义陈述的生效区间与 preferred/normal/deprecated 等级；历史纠正保留记录 |
| **实体解析 / entity_key** | 将源行映射到规范实体；强键校验才可自动合并，歧义交管家或用户；key 不回收复用 |
| **管家决定** | 控制库中可审计的实体合并、否定或拆分决定，必须备份 |
| **控制总数（control_total）** | 由 owner 维护的独立计算路径，与认证指标在固定期间对账 |
| **时间锚点** | 业务日历定义的命名日期边界，答案披露名称与日期；结账边界由授权人员维护 |
| **deprecated / replaced_by** | 公开名称弃用与替代关系；保留足够发布版本并清除引用后才能删除 |
| **`_oo_pos`** | 副本表的系统列：该行最后一次变更的源位点，用于幂等 |
| **CDC** | 变更数据捕获：读取 binlog / WAL 获取增删改 |
| **变更批次** | 管线内部统一的 Arrow 批次格式（op、位点、行数据） |
| **CDC 优先、快照补空** | OO 的首次快照算法：CDC 写入的行不被快照覆盖，快照只填补未变化的行，删除用墓碑保护 |
| **对账** | 按主键分桶比较源库与 Agent DB 的工具与过程 |
| **隔离区（quarantine）** | 表暂停期间原始变更事件的暂存位置（对象存储） |
| **Gateway** | Agent 访问数据的唯一出口：认证、SQL 门禁、代价控制、执行、审计 |
| **`oo_r_<role>`** | 每个 OO 角色对应的 PG NOLOGIN 角色，承载列级 GRANT 与 RLS 策略（禁止表级 SELECT） |
| **claims** | 签发 token 时冻结的用户属性（如区域），经 `oo.claims` 供 RLS 使用 |
| **会话 token** | 按最终用户签发、绑定沙盒、只由出口代理持有的短期 token |
| **外部客户端 token** | 直接接 MCP/SDK/HTTP 的客户端持有的 token |
| **Wrapper** | Gateway 能力的几种接入形式：HTTP、SDK、`oo` CLI、MCP |
| **沙盒** | 每会话一个的隔离执行环境，Agent 在其中工作，无任何凭证 |
| **出口代理** | 沙盒唯一的出站目标；按沙盒身份注入凭证并转发到 Gateway 与模型端点 |
| **harness** | 驱动模型进行工具调用循环的 Agent 程序（默认 Pi） |
| **Agent 入口** | 沙盒外的服务：身份、会话、沙盒编排、事件转发、状态释放 |
| **预热池** | 预先启动、未绑定用户的沙盒池 |
| **状态释放** | 会话结束后吊销 token、删除映射、收集产出物、销毁沙盒等一系列幂等步骤 |
| **使用方（channel）** | 向入口发起提问的系统：网页、飞书、Slack、企业系统 |
| **`AGENTS.md`** | 入口写入沙盒的工作说明，并注入系统提示 |
| **`query_id`** | 每次 Gateway 调用的唯一编号，回答中用于引用 |
| **新鲜度（freshness）** | 某表数据截至的源库提交时间，以及从库延迟 |
| **OO-Bench** | OO 的评测体系（轨道 A–F） |
| **OO-Logistics** | 公开的合成“陈旧物流库”生成器与题库 |
| **pass^k** | k 次独立运行全部正确的比例，衡量可靠性 |
| **dev / holdout / live** | 客户题库的三个集合：日常迭代 / 发布门禁 / 上线后新增 |
| **致命三要素（lethal trifecta）** | 私有数据 + 不可信内容 + 对外通信同时存在时，提示注入可外传数据 |
