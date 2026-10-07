# OO-FE-PAGE-001 页面与用户旅程规格

v1.0；路由和页面权威。继承 [设计系统](design-system.md) 与 [API/状态](api-state-contracts.md)。所有页面当前均planned；路径是浏览器路由，不是声称存在的HTTP API。项目通过选择器/URL query `project` 指定，以后端授权确认；路径对象ID不赋予权限。

## 1. 页面清单

| ID | 路由 | 内容与主要动作 | 后端绑定组 | Layout |
|---|---|---|---|---|
| U01 | `/login` | 企业身份登录、部署名称、返回目标；无明文账号创建 | B01 | Auth |
| U02 | `/` | 选择获准项目/角色，创建对话，输入问题；未就绪原因 | B01/B08 | Chat |
| U03 | `/conversations/:id` | 自己的历史/当前对话、输入、工具进度、引用/结果 | B08/B09 | Chat |
| U04 | `/conversations/:id/files` | 产出清单、完整/部分、大小/hash/TTL、重新鉴权下载 | B07/B08 | List |
| U05 | `/knowledge` | 当前角色发布知识：目录、表、术语、指标、示例、关系 | B05/B09 | Knowledge |
| U06 | `/account` | 当前身份/项目能力、语言/主题、退出；无凭证回读 | B01 | Form |
| A01 | `/admin/login` | 管理入口OIDC登录，普通身份不足明确提示 | B01 | Auth |
| A02 | `/admin/bootstrap` | 一次性初始化/过期/已消费状态，配置OIDC指引 | B01 | Form |
| A03 | `/admin` | 阻断/待办/就绪状态、最近任务、下一步导航 | B02/B03/B04/B08/B10 | Overview |
| A04 | `/admin/sources` | 连接器/版本/模式/健康/新鲜度；添加入口 | B02 | List |
| A05 | `/admin/sources/new` | 连接→只读体检→选表→DBA清单→计划→应用 | B02/B07 | Wizard |
| A06 | `/admin/sources/:id` | 连接脱敏、table/position/quality、预算、暂停/恢复/重快照 | B02/B03 | Resource |
| A07 | `/admin/files/new` | 分片上传、格式、编码/表头/sheet、映射/拒绝策略、预览/计划 | B07/B02 | Wizard |
| A08 | `/admin/files/:id` | 对象版本、校验、导入任务、拒绝、保留/删除/缺失状态 | B07/B03 | Resource |
| A09 | `/admin/jobs` | 任务筛选、owner、进度、失败、可重试/取消能力 | B03 | List |
| A10 | `/admin/jobs/:id` | 持久checkpoint、事件、预算/耗时、重试/恢复 | B03 | Timeline |
| A11 | `/admin/builds` | Self-build运行、来源、模型版本、范围、结果 | B04 | List |
| A12 | `/admin/builds/:id` | S0–S10、cards/evidence、候选、cache、usage/cost、失败 | B04/B03 | Timeline |
| A13 | `/admin/ontology` | datasets/terms/entities/metrics等按领域检索、状态/version | B04/B05 | Knowledge |
| A14 | `/admin/ontology/datasets/:id` | 字段、PK/FK、画像、三层overlay、label/grain/caveats/questions | B04 | Resource |
| A15 | `/admin/ontology/relations` | 关系列表与只读局部图、from/to、基数、证据/审阅 | B04 | Graph |
| A16 | `/admin/ontology/terms` | 术语/同义词/业务定义、映射对象、owner、审核/生命周期 | B05 | List/Form |
| A17 | `/admin/reviews` | 我的待办、类型/领域/owner/影响/不确定性、批量预览 | B04 | Review |
| A18 | `/admin/reviews/:id` | 专家问题、可编辑候选、证据、接受/修改/拒绝/暂缓/转交 | B04 | Review |
| A19 | `/admin/classifications` | 按表列PII/semantic_type与证据、批量确认、显式角色 | B04/B06 | Table |
| A20 | `/admin/ontology/enums/:id` | code/classes/有效期、未知签名/行数、owner确认 | B05 | Table/Form |
| A21 | `/admin/ontology/calendars` | 时区/财年/周期/锚点/结账、旧新值与生效时刻 | B05 | Form |
| A22 | `/admin/ontology/metrics` | 口径目录、owner/单位/锚点/认证与对账状态 | B05 | List |
| A23 | `/admin/ontology/metrics/:id` | 粒度/SQL/enum_filters/tests/control_total/精确diff/owner认证 | B05 | Editor |
| A24 | `/admin/ontology/entities/:id` | members/keys/aliases、候选合并/否定/拆分、金额守恒/历史 | B05 | Resource |
| A25 | `/admin/ontology/examples` | question+SQL、verified/unverified、执行结果和版本 | B05/B09 | List/Editor |
| A26 | `/admin/permissions/roles` | 角色/表列/行过滤/配额，validate-plan-apply | B06 | Table/Form |
| A27 | `/admin/permissions/bindings` | 企业用户/组/claims到project/role绑定、影响 | B01/B06 | Table/Form |
| A28 | `/admin/permissions/preview` | 受审“以用户身份预览”、列/RLS/Pack范围与拒绝 | B06/B05 | Compare |
| A29 | `/admin/releases` | 当前active、候选、差异、回滚历史 | B05 | List |
| A30 | `/admin/releases/:id` | candidate seal、G1–G13、角色Pack、副本屏障/CAS、恢复 | B05/B03 | Release |
| A31 | `/admin/settings/models` | Builder/Agent独立provider/origin/model/SecretRef/驻留与预算 | B07/B11 | Form |
| A32 | `/admin/settings/agent` | harness/版本/预算/AGENTS.md追加配置/渠道/预热 | B11 | Form |
| A33 | `/admin/settings/secrets` | 只写Secret、namespace/版本/轮换/连接测试；无回读 | B07 | Form |
| A34 | `/admin/settings/deployment` | objectstore/retention/timezone/Gitdiff/type-policy/限额 | B11 | Form |
| A35 | `/admin/clients` | MCP/OAuth/client注册/token exchange/能力/撤销；M2 | B06/B11 | List/Form |
| A36 | `/admin/channels` | web/飞书等配置、验签/映射/测试状态；不擅发真实消息 | B11 | Form |
| A37 | `/admin/evaluations` | suites/题库/run/candidate/baseline/public/customer域 | B10 | List |
| A38 | `/admin/evaluations/:id` | scores/n/CI/cost、分类/失败原因、C0–C3、版本diff | B10 | Report |
| A39 | `/admin/sessions` | owner/role/预算/lease/session状态、授权终止 | B08/B12 | List |
| A40 | `/admin/sessions/:id` | 获准回放、SQL审计关联、S1–S11释放证明/资源清单 | B08/B12 | Timeline |
| A41 | `/admin/audit` | action/actor/project/query_id/拒绝/trace/version检索与受控导出 | B12 | List |
| A42 | `/admin/operations` | health/lag/backlog/配额/告警/紧急停止与恢复 | B12 | Overview |
| A43 | `/admin/operations/recovery` | 备份、恢复、升级/回滚计划、兼容预检/运行结果；SP14 | B12 | Wizard |
| E01 | 无权限/404/错误边界 | 入口不足、对象不可见、版本不支持、网络/服务错误、明确下一步 | 全部 | Error |

## 2. 所有页面共用的强制规格

FE-PAGE-01：标题使用业务名称，次行列出project/environment/version；列表必须过滤/稳定排序/分页/选择范围/刷新时间，详情必须对象稳定ID与revision。每页同时提供loading、empty、no-match、error、forbidden、unsupported、stale（适用）、submitting（写适用）状态；不能把403当空列表。后台刷新保留现有数据并标注更新状态，不整屏Skeleton闪烁。

FE-PAGE-02：只在明确有后端能力时显示可执行动作；未接入页面显示“此能力尚未接入”+依赖说明，不显示虚假的成功按钮。普通用户无管理路由能力时不泄漏对象名称/数量。浏览器返回、筛选URL、对象深链、主题/密度、滚动和焦点恢复必须工作。

FE-PAGE-03：公共layout映射：List=标题/动作→筛选→表→分页；Resource=摘要→tabs→数据→右侧上下文；Wizard=步骤/当前表单/固定底部前后动作；Review=队列→业务问题/决定→证据；Release=候选差异→gate→Pack/屏障→执行；Chat=历史→消息→输入→详情。≥1440可三栏，1024两栏，<768单栏；布局尺寸按DS，数据表overflow局部保留。

FE-PAGE-04：所有写动作只调用正式API；页面不写Git文件/SQL/Docker，不改前端数组冒充审批。未知后端字段用绑定缺失提示，不推断新状态。无真实owner的指标只能草案；customer/holdout不进公共浏览器fixtures。

## 3. 接入完整向导 A05

| 步骤 | 字段/说明 | 服务端成功条件与停止 |
|---|---|---|
| 连接 | source名称、connector/真实支持版本、host/port/db/read_from、TLS验证/CA引用、timezone、SecretRef/只写新Secret | connector支持且配置验证；密码不进URL/log/本地保存 |
| 体检 | 只读/版本/复制或批量能力/负载预算/必要对象权限/能力矩阵 | 不足显示精确DBA待办；连接成功不表示CDC可用 |
| 选表 | include/exclude、schema/列类型/估计量/不支持类型/新列默认隐藏 | catalog在授权范围；不能前端编造表名与计数 |
| DBA清单 | 只读建议、必要复制设置/权限、风险和责任人 | 只供复制审核，不让网页对源执行DDL |
| 计划 | sync/consistency/snapshot、预算/typepolicy/拒绝策略、预计步骤与configdiff | validate/plan返回不可变plan/version；估计时长标估计 |
| 应用 | 确认范围/负责人，提交服务端job，显示目标同步与checkpoint | revision/hash一致；冲突重新plan；不自动重试写动作 |

向导中退出提醒只针对未提交表单；Secret未保存值在退出时清理，不能localStorage恢复。跨步骤改变连接/catalog/typepolicy使旧plan失效。暂停/取消实际任务必须等服务端返回cancel状态，关闭网页不暂停同步。

## 4. 文件向导 A07

选择格式→有界分片上传及校验→服务端解析预览→列名/类型/编码/时区/空值/Excel sheet+表头→导入模式与主键/append/upsert限制→计划→应用。标明实际支持格式/版本、文件大小/配额、解析与托管的不同限制；预览来自隔离parser且遵守预算。格式不明、截断、校验失败显示具体原因，不默认CSV；文件名只显示清洗后的label不拼路径。

取消上传触发后端abort并显示清理中；重试只复用有效upload_id/版本；对象完整并hash一致才能供导入。上传完成≠数据导入≠语义发布。导入/用户产出/临时文件分别保留，删除成功必须引用服务端absence/release证据。

## 5. 专家审核 A17/A18/A19

宽屏线框：队列280px | 当前问题flex（至少480px） | 证据320px；窄屏抽屉证据；证据面板内部滚动。头部业务对象+类型+待办状态；自然语言问题置顶，confidence标自评，技术SQL/JSON折叠。

卡内容：问题、candidate label/description/grain/caveats/questions、对象与来源、evidence hash/采集时刻、引用/影响、旧决定、事实stale、owner。证据样本受授权且掩码，数据规模未知不能展示猜测行数。决定：接受/修改后接受/拒绝/暂缓（附缺失信息）；转交owner使用真实assignment能力，能力缺失时提示而不本地假转交。

提交使用review_id/candidate hash/expected revision/idempotency；通过服务端再标已审核，可“保存并下一项”。冲突409/412显示新旧证据+保留当前输入，必须重新决定。拒绝/暂缓需要reason；接受自动提交当前candidate，不自动确认PII、认证指标或发布。键盘前后项快捷键只在非输入焦点有效；批准必须明确按钮，无单键误触。

分类页每列展示pii、semantic_type、rule/model来源、evidence、状态、可见角色；批量预览明确IDs数量/current page，personal/sensitive要求显式管理员范围。reviewer不能认证metric或扩大grants；作者/操作者不能伪造owner资格。所有决定有actor/time/revision/Git审计。未分类字段继续隐藏。

## 6. 业务语义 A20–A25

指标页先业务口径（包含/排除/单位/日期归属/粒度/owner/锚点），后SQL参数/enum_filters/tests/control_total。展示“可执行、对账、owner认证、已发布”四个独立维度；金额/容差/diff精确值不经Number。认证要求后端owner与对应输入hash/G5a/G5b，按钮不能只改certified布尔值。语义diff保留字面量/日期/状态/阈值变化；弃用/替代/删除引用影响可查。

码表页保留unknown/undetermined，不把新码塞进已有业务类；valid_from/to与历史as_of可查，统计权限受控。日历页时区/财年/闭账期/锚点，结账前预览旧新值/生效时间并记录actor。实体页强键/弱候选区别、合并与否定pair/拆分/members/alias有效期、金额守恒、隐藏成员不可泄漏；图形不能直接拖节点触发合并。术语页definition/synonyms/maps_to/owner/review；示例页question/SQL/执行版本/verified与结果hash变化说明，未验证默认不进Pack。

## 7. 发布 A29/A30

候选revision/input seal→与active版本差异→受影响用户/角色/名称/示例→逐项G1–G13（含G5a/b,G7-pre/post）→角色Pack manifest/digest/预算→授权fence/副本屏障→CAS active。每项pass/warning/block/not_run/pending显示报告引用，不能把未运行显示通过。强制发布仅后端列明的非安全可豁免gate、合法owner理由/期限，不能暴露万能force。

执行后按真实checkpoint显示进度；browser断线重读job与active版本，不能因动画完成显示已发布。回滚展示语义/权限影响与后端允许动作，不恢复已撤销旧能力。API未支持公共发布时，本页只能预览candidate，按钮不可执行。

## 8. 用户对话 U02/U03

左历史/新对话；顶部project/data space/current role/knowledge版本与就绪状态；消息列宽约760px，表格结果可展开全屏。下方输入多行、发送、停止本轮；停止本轮与结束会话分开。只有后端实际角色/Pack/隔离就绪才开始，普通身份不能选任意role。

回答结构：自然语言→业务口径/时间锚点→真实结果table/chart→query_id引用→freshness/质量警告→文件。工具事件显示简短动作/运行/完成；SQL详细信息按能力展开，不输出内部思维链。来源SQL由真实query证据获取；伪query_id不能点击，外部链接不自动预览；所有warning保留。

entity歧义显示服务端允许的候选选择，not_found明确，拒绝不改写成无数据。截断明确部分结果与限制，提供获准导出路径；没有完整receipt不显示“已完成”。WebSocket重连按sequence追补/快照，不重新发送旧prompt；turn completion不同于session closed。关闭会话显示“访问已撤销/资源释放中/释放确认”的真实状态；S1–S11未证不伪closed。

历史/outputs重新校验当前权限；旧tuple失配初版拒绝恢复，说明用户可新建会话而非继续使用旧高权限结果。文件显示TTL/hash/大小/完整性，下载由当前权限校验；403/过期不复用旧URL。User看到仅自己的记录；管理员回放是独立审计授权入口。

## 9. 配置/运维/评测

模型配置明确Builder与Agent两个区块，provider/origin/model/SecretRef/timeout/request-token-budget/驻留与samplepolicy独立；API Key只写、保存后状态脱敏，不回显。测试模型请求有显式范围/费用说明，使用公开/获准数据且不将成功HTTP当生成已验收。角色变化走validate/plan/apply并预览受影响会话。

Settings对象存储、retention、Git、hooks/plugin只呈现既有安全配置/不可变制品；禁止网页上传任意代码在线构建/执行或任意源DDL。外部client功能标M2；飞书测试不自动发真实用户消息。评测显示样本n/置信区间/gold版本/dev vs holdout vs live隔离、C0–C3；客户私有结果按授权，不暴露holdout gold供调优。运维保留新鲜度/复制延迟、拒绝、ACK/位点、任务、审计失败、资源释放指标。

恢复向导选择已验证备份→兼容性与影响预检→负责人确认→job→恢复证据；只依赖服务端能力。紧急停止与恢复为两个明确操作；管理员全局停止不依赖模型，不因UI按钮成功返回假设资源已消失。
