# 当前短期目标：接库 → Self-build → Pi 真实问库

**最新更改时间：2026-10-09 09:46:55（Asia/Shanghai，UTC+08:00）。** 后续每次修改本文必须同步更新此字段。

2026-10-08，用户明确调整优先级。完整主 Spec/Plan 仍保留，当前里程碑按本文件排优先级，不能宣称整个项目完成。

## 默认路径与依据

依据 docs/09-sandbox-and-agent-runtime.md §6.2：默认 Pi RPC，内置 read/bash 工具读取 /knowledge 文件夹，通过 bash 调 oo CLI，经 Gateway 查询，不依赖 MCP。MCP 属于外部客户端扩展，继续 TODO，不能成为此次 Pi 闭环的前置。Pi 不直接获得数据库连接或模型密钥。

一条命令接收获准源库连接信息/私有配置 → 只读体检与复制计划 → 有界一致性复制到本地 PostgreSQL Agent DB → 本地数据完整性校验 → 封存导入版本并断开源库 → 仅对本地副本执行 catalog/profile → 真实 Builder LLM → 标准 OOS YAML/generated 与实际审核状态 → 生成本地 Knowledge Pack 文件夹 → 启动 Pi RPC，先读 README → Agent 自己编写 SQL，用 oo query 调实际 Gateway → 返回有 query_id/真实审计与精确结果的答案 → 关闭并确认会话资源释放。

## 当前交付范围

- 单用户、本地明确 operator development 模式；数据库连接和模型密钥为私有文件引用。企业源资料尚未提供，不连接其测试账号，复用本地私有十万行PG。
- 输入 database type、host/port/db/TLS/SecretRef 和获准复制范围，检测支持情况并真实连接；必须先完成源库到独立本地 PG 的复制与校验，随后 Self-build、Pi 和 Gateway 仅使用该本地副本。首先完成 PG 全闭环；“任意数据库”仅在对应真实连接器通过同闭环后支持，未支持返回明确错误，不能伪称全部兼容。其它 Plan 数据库与文件支持仍保留 TODO。
- Self-build S0–S10质量/来源/模型缓存和失败路径保持原验收；可用结构说明和源声明事实先形成未认证draft Pack，未知和未审核推断如实标注。不能自动将LLM草案标approved、捏造人工审核/Git发布/owner认证。
- 单用户会话可以明确使用本地draft Pack做开发预览，若支持该模式须独立显式配置；它不是active生产发布。正式“YAML→overlay→Git→发布→AgentPack”继续完整主Plan TODO，不阻塞当前明确开发预览，但不能报为已完成。
- Gateway继续真实只读parser/guard、结果精度、预算、超时取消、审计与Secret边界；不得以新的私有SQL模拟server替代现有Gateway实现。认证的多用户管理与角色分配延后，不取消基础只读能力。
- 本地Pi development process须如实标非runsc生产sandbox；production缺runsc仍fail closed。预览模式不伪装发行认证。

## 源库到本地副本：短期前置必做链路

本节补齐原短期计划的遗漏：直接对预先装载的本地 fixture 读取 catalog，或导入上传文件，不能证明“输入源库连接配置即可先拉到本地”。MP-01a–d 是 MP-02–06 的必需前置。

### 依据与模式边界

- [源连接与摄取](../docs/03-sources-and-ingestion.md)：真实 Connector 能力、只读体检、稳定块规划、类型映射和读取预算；对应 Spec REQ-SRC-01–04、Plan SP03-W01–05。
- [管线](../docs/04-pipeline.md) I-PL-1/I-PL-3/I-PL-5、§4 与批量一致性规则：管线独占源凭证、只读分块、目标 COPY、部分复制不公开；对应 Spec §8、Plan SP04-W02/W03/W05/W06。
- [Agent DB](../docs/05-agent-db.md) I-DB-1–4：独立 PostgreSQL 保存同步数据，Gateway 不连接业务源；标准部署查询只读副本。单实例 development profile 必须明确披露，不冒充主从部署验收。
- 原冻结 SRC-D01/E43 的“只读源 API→本地 PG→断源本地读取”要求及 a335 复审仍有效。本节不是放宽既有单连接预算的批准。

短期先交付显式 `full` 的一次性 PG 只读一致快照复制；该模式不宣称持续同步、CDC追平或最新业务状态。CDC-first/fill、完整位点屏障等仍按原 Plan 实现与验收；不能将一次性导入标成 CDC `live`。不能证明一致切面、超过预算或源模式不支持时，返回未完成/明确拒绝，不静默降级后宣称完整。

### 顺序与输入输出

1. **连接与计划：** 用户提供源类型、主机、端口、库名、TLS要求、私有 SecretRef、获准表/列范围和预算。执行不试写的权限体检，记录实际支持模式、规范名映射、类型策略、稳定键/块规划和复制量估算；未知数据库或不支持类型明确拒绝/隔离，不能无损性未经证明就转成文本。
2. **只读复制：** 源数据和 catalog 在同一可证明一致的快照范围内读取；PG 使用 `REPEATABLE READ READ ONLY`，数据有界流式传输至独立本地 Agent DB 的 LOGGED shadow generation。源库不执行 DDL/DML、写 heartbeat、权限变更或试写权限探针。批次在目标提交后才记录完成，重放同批不得重复写入。
3. **完整性校验：** 逐表核对所选范围的 schema/type mapping、行数、稳定键覆盖及 canonical typed 内容，覆盖 NULL、int8、精确 decimal/scale、时间与支持的二进制类型。必须对齐本次源快照的已封存证据；源库继续变化时不能用另一时刻的 COUNT 冒充本次一致性证据。行数相等单独不足以通过。索引/ANALYZE 按原 Plan 在目标完成。
4. **封存与切换：** 只有全部获准表和校验完成，才产生不可变 manifest，记录源身份脱敏指纹、源快照/一致性模式、schema/config/type-policy版本、目标数据库身份、generation、逐表行数/校验hash、完整与失败项、实际预算、起止时间及本地连接配置路径。源失败或部分成功不得返回完整/可问库状态。
5. **本地后续流程：** 关闭源连接，后续配置只提供本地库 SecretRef 和导入 manifest。本地保留原始 catalog 名称与逻辑映射，Self-build 的来源是该导入版本；Builder、Gateway、Pi、问答测试不得获得源凭证或向源库发出查询。离线副本的数据时间/快照窗口必须展示，不能误称持续新鲜。

### 预算、恢复与数据托管

- 沿用原 SRC-D01 冻结预算：源端数据读取与 catalog **总峰值1个连接**、传输批次最多1000行、`statement_timeout ≤ 30s`、`lock_timeout ≤ 1s`、一致快照最多300s；这些是本短链路的已冻结上限，不将其误写为全部管线通用默认。额外全表扫描/统计须单独纳入读取预算，不使用 EXPLAIN ANALYZE 加压源库。
- 现有 local-pg-mirror 使用两条源连接的已知复审缺陷须修复并独立复验；不能隐藏第二条连接、删除 catalog 保证，或用本节静默变更预算。
- 一致快照连接失效、超时或 schema 漂移时，不混用新旧快照续拷并宣称一致。按 Plan SP04-W06 废弃本次未验证 shadow并建立新run，或复用已完整验证的耐久spool；恢复须绑定正确输入版本与generation。取消停止源后端，局部数据不成为可查询副本。
- 企业数据和完整原始 catalog 仅放本地私有目录/受限PG卷；配置只记录私有密钥文件引用。明文账密、原始企业数据不进入 Git、公开证据、argv、日志或模型请求。模型仅按 Self-build 已获准证据范围使用本地副本；数据驻留许可保持原规则。
- `close` 释放会话、代理映射与临时资源，不默认删除用户源库或持久本地副本。副本清理须显式目标范围和授权；失败恢复不能覆盖其他导入版本。

### 必须新增的真实验收

| ID | 操作 | 通过条件 |
|---|---|---|
| MP-01a | 从私有连接配置发起复制 | 真实独立源库与本地目标库；来源和目标身份不同；支持/权限体检、计划及脱敏配置可复核。先从PG证明，不虚称任意数据库已兼容 |
| MP-01b | 复制十万行（三表2000/8000/90000） | 所选完整数据实际写入本地PG；逐表规范值/hash、NULL/精度/类型、映射和键覆盖一致；测量源只读零写及总连接峰值1，不用文件上传或预装目标fixture代替复制 |
| MP-01c | 故障、取消、超时、漂移、重启/重放 | 无伪complete、无部分可见、无源改写、无旧新快照拼接、无重复目标副作用；有明确失败/恢复状态和资源/后端停止证据 |
| MP-01d | 封存后阻断所有到源库的访问 | 在源库不可达且源凭证不提供给后续组件时，使用同一本地副本成功完成Self-build、本地Pack、Pi→CLI→代理→Gateway问答；每次查询审计指向本地数据库与同一导入generation，源访问计数为0 |

独立公共fixture可在隔离资源中断源验证，不擅自停真实企业源库。验收证据包括复制manifest、逐表typed对账、读取预算/源访问测量、断源前后运行结果、query_id/目标实例关联、原始退出及固定提交SHA；不能仅靠配置字符串声称“local_only”。

实现中、作者测试通过、固定独立复审通过和短期端到端通过分别记录。此次文档补充不声明复制实现已完成、不增加已验收计数，也不自动派遣或恢复任何worker。

## 必须一次完整验收

| ID | 真实输入与行为 | 证据 |
|---|---|---|
| MP-01 | 源库连接配置→只读复制→本地校验→断源后本地 catalog/profile | MP-01a–d 全通过；独立源/目标身份、源读取预算、逐表 typed 对账、manifest/版本hash、断源后零源访问 |
| MP-02 | 真实grok4.7 self-build→标准YAML/Pack目录 | 模型fresh请求/缓存/tokens/cost状态、parse/schema/seal、README/tablefiles |
| MP-03 | Pi先read Pack，再自行bash编写/提交SQL到oo CLI→实际Gateway | 真实Pi RPC工具event、工具SQL、Gatewayquery_id/审计；提示不得提供答案或预写SQL |
| MP-04 | 至少count/关联/条件聚合/null与精确数值真实问答 | 冻结问题独立gold、答案与真实行对应，不只HTTP200或人工proposal |
| MP-05 | 新会话关闭、坏连接/模型失败/SQL写入拒绝 | 无伪成功，无破坏源库；超时取消和资源absence |
| MP-06 | 可复现启动与即时demo | 完整命令/config示例（无密钥），私有fixturecleanstart、复跑复用cache，固定提交/原始退出/全证据SHA |

作者只有MP01–06真实通过才交付；未通过报告明确阻塞，不缩为生成文件或读Pack不查库。Codex验收固定提交，独立必要差异复审，不扩无关admin矩阵。

## 强制执行规则：高并行与完整收敛

**保证尽量高的 Grok 实际并行工作量，保证多线并行；没有完整实现并验收整个短期目标，不允许主动结束工作或宣布交付。**

1. 主协调必须主动拆分互不冲突、可独立验收的工作线，持续并行推进源连接/只读复制、本地校验、Self-build、Pack、Pi/CLI/代理/Gateway、关闭释放、测试及固定提交独立复审；不得只盯住一条线，让可推进的其他工作线闲置。
2. 在真实可并行任务、依赖和资源允许时，尽量维持至少15条实际运行的 Grok 工作线；主线开发并发上限20，主线独立复审不占开发槽。不能用任务卡数量、已退出会话或尚未启动的计划冒充实际并发，也不能为凑数量制造无关工作、重复worker或重复验证。
3. 高并行不得造成代码、会话或资源冲突：明确唯一写入边界，冲突任务串行，消费者使用固定提交。全局Rust builder上限10、每任务最多1；Node和浏览器各全局上限2、每任务最多1。前端仅复用原G1/G2/G3三个固定Grok会话，含独立复审，不新建第四会话。资源满时排队，不提高资源上限绕过约束。
4. 继续采用一次10分钟等待后统一观测与验收，不反复轮询；任务真实退出后核对实际阶段结果、固定源码、原始测试退出、报告与hash，再决定续接或派遣。续接先收已有资源，不重复启动仍运行的builder/PG或同UUID进程。
5. 工作退出门槛是MP-01a–d及MP-02–06全部真实通过，且固定提交、独立审阅、可复现启动、端到端测试和证据齐备。局部模块通过、代码已提交、HTTP200、draft Pack已生成、CLI退出0或部分问答成功，均不能代替完整短期目标。
6. 没有完整实现整个短期目标，不允许以阶段总结、进度报告或“后续再接”主动终止持续工作。状态回复可以说明进度，但不能当作完成或取消剩余任务。用户明确暂停/取消、工具中断、额度耗尽或必要外部条件阻塞时，如实记录未完成项与恢复入口，不伪造完成；条件恢复后从原任务状态继续收敛。
7. 每次工作前核对对应docs、Spec和Plan条款，持续更新总台账、短期TODO、依赖和实际并发；只有完整验收通过才勾选完成，不通过缩小范围或改变分母达成表面完成率。

## 所有权与优先级

P0：源连接与复制链（a383已有接入接口、local-pg-mirror复制能力；主协调必须明确实际复制修复与固定复审负责人，不能默认a383已覆盖）；a373完整selfbuild；a397真实Pi+Gateway；a405一命令本地接库/构建/对话用户流程编排。a399兼容、a389真实100k问题基准为P0支持线。
P1：最小可运行打包、取消/释放与复现证据。前端只在该路径需要时接最小对话，完整admin排后。
P2保留：多用户权限/管理角色API/admin完整页面/发布控制API/Secret管理控制台/外部MCP OAuth/完整运营与发行审批。正在运行的这些任务允许收当前安全固定交付，不启动重复的新功能；没有用户数据的工作不得因优先级降后被删除。

完整主Plan与OC01–06仍待完成；当前验收不改变其完成定义。当前局部79/90不是本里程碑完成率。

## 用户确认的默认 Pi 路径（短期必做）

用户于2026-10-08明确确认：先完成默认Pi，不先做MCP。

实际调用链：入口 stdio RPC → Pi read /knowledge/README.md 与表/关系/指标文件 → Pi bash 调 oo CLI提交模型自行生成的只读SQL → oo HTTP到固定出口代理 → 代理注入会话凭证并mTLS到真实Gateway → Gateway校验/预算/只读执行/审计 → 精确结果、warnings、truncation、query_id → Pi答案引用 → cancel/close释放。

代理持有Gateway会话token与模型SecretRef，Pi与CLI不获得数据库密码或可复用会话token。development本地部署边界须明示，不能因为MCP延后省略真实Gateway/parser/guard、查询审计和取消；生产runsc等门槛保留原Plan。

MP-03额外证据：真实Pi read与bash工具事件、oo CLI真实HTTP、代理request关联/注入及mTLS记录（无密钥）、Gateway实际query_id/SQL审计、模型答案引用同query_id；直接数据库驱动/假Gateway/预写SQL/人工proposal均不满足。MP-05包括proxy会话映射与凭证失效/资源关闭的实际证明，缺资源不得伪closed。

MCP stdio/StreamableHTTP/OAuth相关开发与复审延期到P2，不作为短期依赖。当前a388若仍有自己构建可收安全固定成果，不再重复启动MCP新功能；代码与原SP13验收TODO保留。

## 当前执行 TODO（2026-10-09；不替代验收）

- [ ] MP-01a–c：复制作者固定提交 `5df69049267bf16eca36d311a1b3082f27aa9754`，独立实测已报告通过；待独立固定提交与协调核验。失败事实标签修复 `c166fd11d2c32b00475bce4e85e6d98ba6a256f0` 单独记录差异复审。
- [ ] MP-01d：在源被阻断时，把封存 bulk import 与 Self-build、Pack、真实 Pi/Gateway 审计绑定，证明同一目标身份、generation 和零源访问。
- [ ] MP-02–04：真实模型 fresh/cache 与成本状态、标准 YAML/Pack、Pi read/bash 自主 SQL、冻结问题精确问答全部实证。
- [ ] MP-05：失败、取消、写入拒绝与真实关闭释放完整证明。局部关闭成功不代替完整验收。
- [ ] MP-06：一命令从源配置复制后进入问答；固定源码、可复现启动和完整独立验收。

当前唯一写入负责人：local-pg-mirror 为 a410；minimal-product-loop 为 a411（接替已退出 a405）；agent-entry 为 a412（接替已退出 a397）。复制复审 a409、完整验收 a407；原失败证据和已有资源保留。每项仍未勾选完整验收。

当前调度补充：一次等待已按用户要求改为10分钟。短期 Pi 实跑唯一产品负责人为 a411；a412 的 HTTP/session 扩展保留主 Plan TODO，不要求重复跑第二套六问。Pi harness 合法事件兼容固定 c722c037，由 a414 窄范围独立复审。完整验收 runner 由 a415 接替已退出 a407，原夹具、gold 与证据保留；按实际执行依赖固定源码，不把整个 MP-01 已签作为验证 MP-01d 的循环前置。MP-01a–d 与 MP-02–06 的实际行为标准不变。

## 当前用户新增：物流完整验收（2026-10-09）

- [ ] a419 单一完整集成负责人：独立 Docker 物流源 PG（hubs/shipments/tracking_events 共十万行）→用户连接配置→只读复制至独立本地 PG→Self-build→标准 Pack→真实 Pi/CLI/代理/Gateway→六题精确答案与查询审计→关闭。不以预装目标替代复制，不向模型提供金标准或 SQL。
- [ ] 逐阶段核查 S0–S10 实际输入、输出、审核身份与证据；开发未审核预览不等于正式审核/发布，缺失阶段如实记录并修复，不宣布已完整走完。
- [ ] a419 准备并实现完整验收与独立预计算六题 gold；现有417/418/416交付后，协调显式转交冲突产品路径给419修复关键缺口，禁止同时写入。

新增优先级：权限扩展、admin 与 MCP 保持 P2，先完成可运行闭环。当前负责人411配置/run/caller（idle固定073dab30c），416 ask/main/gold，417 UID harness，418 pack.rs，419独立物流验收和集成。原415 runner保留但不同时启动重复全量验收。所有完整验收勾选保持未完成，直到实证通过。

### 物流统一集成与阶段核查

419已接管全部 minimal-product-loop 与部署路径，411/416/418不再续写。现有固定模块：配置/源断连证据073dab30c，UID harness ef98dcde（独立复审bb9f36e），标准draft Pack5b239202（独立复审7bf7a2a）。物流fixture/gold fa45c6a只证明源库十万行和独立答案，尚未证明产品复制或六个模型回答。

固定阶段核查4061bd07表明当前入口不完整：S2语义类型、S3关系推断、S4码表识别需接入 selfbuild-rules；S6实体聚类、S7指标候选、S8示例候选需接入 selfbuild-suggestions；S9审核队列和S10列分类须保留实际审核/分类身份。S10不是发布；后续Git与发布仍为独立主Plan工作。不得从S5 validated或生成Pack推断全阶段成功。新的实际阶段结果由419统一集成后验收。

- [ ] 通用物流连接配置的一键复制/构建与实际六题全部通过。
- [ ] S0–S10逐项实际阶段证据齐备；缺失/人工门槛分列，无伪approved。
- [ ] 模型fresh/cache/失败、源断连、Gateway审计和close共用同一实际导入代次。

## 最小 PostgreSQL 草案闭环：固定验收（2026-10-09）

当前结论以本节为准；前文未勾选项为当时历史状态。源码3b70b151（实现fb0ea042）、证据34d14ade；独立协调复审见 execution/reviews/gpt-minimal-closure-fixed-review.md。GPT-6.1 Sol low直接完成开发，暂停新Grok派发。完整主Spec/Plan没有完成，不改变其分母。

- [x] MP-01a/b/d：真实物流PG连接配置→独立本地PG十万行复制、typed manifest→断源后本地Builder/Pack/Pi/Gateway，同一实际导入身份；原复制故障子验收沿用已固定a409，新增实际坏连接/取消/失败记录。
- [x] MP-02：真实grok-4.7 fresh/validated cache及unknown成本记录、标准原生OOS草案和实际Knowledge Pack；模型超时/401失败不ready。此项仅未审核预览，不是正式审核发布。
- [x] MP-03/04：真实Pi先读Pack、自写SQL，通过CLI/代理/mTLS/nativeGateway，物流六题exact cells与model cited query IDs全部通过。
- [x] MP-05：SQL写入拒绝、真实在途取消、关闭/释放、released ask拒绝，本地持久库保留。
- [x] MP-06：可配置的一条launcher启动、实际空target首次复制与模型失败恢复后Pi smoke/close，缓存新会话同SQL不同ID复问；475证据SHA核验通过、代码固定提交。
- [ ] 正式S9人工审核、S10列分类及正式控制服务全阶段状态完成。S2–S8已实际运行候选工具，不冒充正式服务checkpoint。
- [ ] 正式审核overlay→Git版本→发布→Agent Pack、多用户/admin/API/MCP/其他数据库和完整发行门槛，继续主Plan。
- [ ] 企业连接资料到达后，用获准类型/范围/TLS及预算复制并基于本地库验证；本轮未连接企业测试库。

入口为deploy/logistics-demo/COMMANDS.md与linux/closure/launch-run.sh；run仅为隔离后的runtime构建/恢复入口。支持PG且受类型/表范围/读取预算和现有SQL子集约束，不宣称任何数据库均可接入。当前自有运行时已停，原持久数据保留。
