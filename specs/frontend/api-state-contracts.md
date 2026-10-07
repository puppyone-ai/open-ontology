# OO-FE-API-001 API绑定、身份与状态契约

v1.0；这是前端接入规范，不宣称管理接口已经存在。当前正式行为以主Spec、服务的发布OpenAPI/JSON Schema和实际固定提交为准。字段示意若与正式Schema不一致，必须更新绑定记录和对应页面，不能在TS中伪造。

## 1. 绑定登记与后端完整接入

FE-API-01：每组操作在编码前填写binding：group、operationID、真实method/path、服务/版本/commit、request/response Schema hash、capability、subject scope、revision方式、error映射、event契约、goldenfixture、实际测试出处。`unbound → schema_frozen → fixture_verified → live_verified`分别记录；fixture_verified不得对外称实际功能可用。组编号B01–B12是工作包引用，不是新API路径。

| 组 | 必需后端操作/对象 | 主Plan依赖 | 必须返回/确认的关键事实 |
|---|---|---|---|
| B01 Identity | OIDC/bootstrap/session/current identity/capabilities/projects/role bindings | SP02-W04/W05,SP11-W01 | 服务端subject/project、入口能力、真实role/claims tuple、过期/撤销 |
| B02 Sources | connector matrix/config validate/read-only inspect/catalog/select/DBA plan/apply/pause/resnapshot | SP03-W01–W05,SP04,SP02-W03 | 支持矩阵、source/config revision、预算、计划hash、jobID、权限体检 |
| B03 Jobs | list/read/event resume/cancel/retry/checkpoint/result | SP02-W01,SP04,SP11 | generation/sequence、持久status、budget/deadline、可用action、result证据 |
| B04 Builder/Review | S0–S10/catalog/profile/cards/generation/candidate/decisions/classifications/assignments | SP07-W01–W05 | provenance/confidence/evidence/input hash、scope、review revision/actor、unknown |
| B05 Semantics/Release | terms/enums/calendar/metrics/entity/examples/Pack/candidate/gates/publish/rollback | SP07-W06,SP08-W01–W06 | owner身份、精确对账、semanticdiff、immutablecandidate、gate报告、active版本 |
| B06 Access/Clients | role/config validate-plan-apply/preview/token metadata/revoke/OAuth exchange | SP02-W05,SP06,SP13 | targetsubject/current tuple、server preview、audit、单次token显示能力 |
| B07 Secrets/Objects | Secret write/rotate/status/test; upload parts/complete/abort/object/download/delete | SP02-W04,SP05,SP11-W03 | 只写SecretRef、不返回值；对象版本/hash/size/TTL/absence/currentauthorization |
| B08 Entry/Sessions | own conversations/session create/turn send/abort/close/events/history/restore/admin terminate | SP10,SP11-W01–W04 | owner、current tuple、turn/eventID、真实completion、S1–S11 release证据 |
| B09 Gateway | tables/describe/entities/values/query/validate/freshness/knowledge/export/gap/citations | SP06,SP09 | 数据权限、typedrows、queryID/audit receipt、warnings/truncated/freshness |
| B10 Evaluation | suite/run/dev-holdout-live/result/compare/error/cost | SP12 | public/private域、gold/report版本、n/置信区间、封闭holdout、评分证据 |
| B11 Configuration | Git validate/plan/apply/diff/models/harness/AGENTS追加/channel/deployment/plugins | SP02-W02/W03,SP10,SP11-W06,SP13 | 原/候选revision、schema/model/prompt/version、驻留、planhash、audit |
| B12 Operations | health/lag/backlog/audit/session replay/release/emergency stop/backup/upgrade/recovery | SP11-W06,SP14 | 当前状态与时间、授权scope、复原计划、确认checkpoint、验收报告 |

管理B组尚未绑定时，可以publicmock开发视觉，但真实路由必须显式unsupported/unavailable，写按钮无可执行action。后端缺API时登记最小请求到协调者；由Rust owner拥有服务代码，页面Grok不跨目录自建API。

## 2. 已有设计的数据面端点引用

以下来自docs/08，不证明当前运行服务全部支持；B09必须按actualfixedservice逐项绑定，不能从CLI存在推导HTTP就绪：

| 操作 | Spec设计端点 |
|---|---|
| 表列表/详情 | GET `/v1/tables`; GET `/v1/tables/{t}` |
| 实体/值查找 | POST `/v1/entities/{e}/resolve`; POST `/v1/values/search` |
| 查询/验证 | POST `/v1/query`; POST `/v1/sql/validate` |
| 导出 | POST `/v1/export`（Arrow流与完成协议） |
| freshness | GET `/v1/freshness` |
| Knowledge | GET `/v1/knowledge/{path}`; POST `/v1/knowledge/search` |
| 知识缺口 | POST `/v1/feedback/gap` |

Loopback实验的`/v1/sql`返回501不等于正式query已经实现。网页Agent经Entry/harness执行，浏览器不能把Secret发到Gateway伪造身份；明确权限的手动查询编辑器仍使用正式Gateway认证。MCP/OAuth按M2适用，不引入自行拼token的兼容层。

## 3. 身份、Secret与浏览器边界

FE-API-02：OIDC Authorization Code流程与服务端会话是默认；浏览器导航入口不传密码/claim自行认证。以服务端current identity/capabilities决定UI；无admin能力的subject跳错误页，不能因为地址/admin信任其管理身份。用户数据role与管理role不同，后端根据绑定验证。初次安装一次性链接不可复用且消费后去掉URL敏感token。

建议同源BFF/Entry提供HttpOnly/Secure/SameSite合适的会话cookie，所有cookie认证写操作落实服务端CSRF/origin防护；若现有认证架构不同，B01 freeze记录唯一适配方案，禁止为了前端方便将长期token放localStorage。跨域/callback/returnTo仅同源允许路径，拒绝开放重定向。OIDC自身不是Secret读取权限。

API Key/数据库密码表单输入只在当前组件内存，HTTPS写入Secret服务；不进路由、日志、analytics、mutation devtools、截图或持久cache。write成功后清输入，只显示configured/SecretRef/rotationtime。空输入表示保持，explicit clear单独动作。Secret读取值不成为前端能力；合法一次性外部token显示需服务端一次返回且明确复制/退出后不可再读，不能默认所有token可回显。

前端权限不是安全边界；后端每请求/下载/历史恢复/预览重新校验；跨project objectID返回拒绝且不泄漏对象存在。身份失效后清相关数据、WebSocket/queries/cache，转入口登录；请求失效不自动丢未提交非敏感编辑，重新登录仍必须比较revision。

## 4. UI view-model 契约（不是强制后端DTO）

FE-API-03：Api adapter将发布DTO映射为view-model，保留所有业务信息和unknown；禁止`as any`掩盖Schema不符。以下概念字段需要绑定，不要求服务端同名：

| 对象 | 需要的概念字段 |
|---|---|
| ResourceRef | stableID/project/revision/schema_version/updated_at/provenance/allowed_actions |
| ReviewItem | kind/objectref/candidate_hash/input_hash/evidence_refs/confidence/reviewstate/owner/impact/previous_decision |
| DecisionCommand | itemID/candidatehash/expectedrevision/decision/editedpayload/reason/idempotency |
| JobView | jobID/generation/seq/status/checkpoints/deadlines/progressnumerator-denominator/estimatedETA/cancelstate/resultref |
| ReleaseView | candidate seal/baseactiveversion/gate reports/role manifests/fence+replica checkpoints/activeCAS/outcome |
| ConversationView | owner/project/session+turn IDs/policy+knowledge/catalog tuple/events cursor/current permitted actions |
| QueryReceipt | actualqueryID/SQLref/hash/session+turn binding/typedrows/complete-truncated/warnings/freshness/auditref |
| ArtifactView | objectID/version/sessionowner/pathlogical/size/hash/mimetype/complete/expiry/downloadcapability |
| ErrorView | safe code/message/field errors/retryable/traceID/currentrevision（许可时） |

API不同版本未支持某字段时展示缺失说明，不能编造actor/time/cost/owner。批量决定的partialsuccess必须逐对象呈现，不以HTTP200推导全部成功。

## 5. 双状态机与并发

FE-API-04：请求状态idle/loading/refreshing/error/submitting，与review/job/release/turn领域状态分离。领域状态严格绑定后端枚举；新增未知枚举显示“版本不兼容”并保留safe rawcode，不默认为success。

| 情况 | UI必须行为 |
|---|---|
| 写请求在途 | 防重复并保持输入；disabled带理由 |
| 超时/断线、结果不确定 | 显示结果待确认；按operationID/job/ref查询，不盲重发非幂等写 |
| 409/412 revision冲突 | 加载授权范围内新版本，展示diff、保留编辑，重新确认 |
| model失败/输出不合法 | 结构草案/具体refusal；不产生审核批准或发布状态 |
| browser fetch abort | 仅停止该请求；任务状态不变；真正取消调用对应API |
| 取消任务 | cancel_requested→后端确认canceled/仍运行/失败，字段名按schema绑定 |
| 会话释放 | access blocked可先显示，closed只在后端资源不存在证据确认 |
| 发布未完成 | 仍候选/执行中/失败；active版本来自服务端，不由UI进度推断 |

mutation不得乐观更新review approved、PII、role grants、metric certified、active release、artifact complete。非敏感主题/列宽允许本地更新。expectedRevision/If-Match/CAS按后端正式契约；没有并发保护的危险API必须登记阻断，不能用按钮disabled代替后端互斥。

## 6. 流、重连与精确结果

FE-API-05：网页对话按主文档WebSocket；管理jobs使用后端支持的事件流（SSE/WS明确binding）或有界退避查询。事件需要持久sequence/eventID、subject/session/turn/generation、kind/payload和确认语义；不能让前端新定字段冒充已存在。重连携带最后确认cursor，重复事件去重，缺口请求授权snapshot；generation变更不能拼入旧消息。浏览器refresh从后端恢复，不自动重放发问。

stream EOF/连接关闭不意味着completion。turn结束须terminal事件/receipt；完整回答与工具query/export完成分别判断，tool失败保留warning。cancel不关闭旧数据授权自动成功；session revoke/释放执行后端证明。可恢复的无进展断线按backoff停止高频请求，页面明确重连/手动重试。

FE-API-06：int8/decimal用字符串或typedcoef/scale；Number仅用于已明确安全范围的图轴近似，tooltip/下载/对账保留精确值并说明近似。NULL独立，空串保留，bool不当文字猜测。schema/order/encoding由receipt提供；截断标partial、返回行数/限制/bytes；Arrow导出必须完成meta/hash/rows对上，断流不能返回成功下载。CSV/Excel公式安全依后端typedexport；前端不私改负数/数字作为转义。

FE-API-07：所有Markdown/表格/SQL为安全文本，禁任意HTML/script/iframe/SVG active/远程图片/自动link preview。链接显式用户动作且protocol限制；query引用服务端验证owner/currentrole/tuple/真实receipt。转为HTML的可信chart也只平台固定组件，不能执行model代码。

## 7. cache、预算和正式接入验收

FE-API-08：querykeys按subject/project/currentrole/policy/knowledge/catalog版本及参数（适用）隔离；旧请求的late response不能写入新项目上下文。关闭Dialog/换项目传递AbortSignal，卸载WS监听；token/claims不进devtools持久日志。默认无敏感持久cache/offline回答。403/撤销清相关cache，不能继续显示旧高权限数据并假称缓存命中合法。

每B组验收一份binding表+OpenAPIhash+public golden request/response+必要401/403/409/unknownstate+actualfixedservice receipt；前端集成不可因loopback可信operator测试直接签publicauth。完整旅程必须正式OIDC/管理RBAC/PG授权/发布/harness，外部条件独立保留。mock测试只证明UI状态，不证明服务产品能力。
