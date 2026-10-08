# 前端统一执行台账

唯一派发依据：[总任务卡](master-task.md)。Codex 唯一维护；旧 28 卡仅为范围参考，不再逐卡派遣或追加状态。更新：2026-10-08，Asia/Shanghai。

| 固定角色 | 固定 UUID | 实际 handle / 状态 | 已交付 source / review | 唯一所有权 / 前置 |
|---|---|---|---|---|
| G1 基础与用户端 | eef831aa-dba4-4af2-a78b-541bc9e0a351 | 42805，新续接；原45339已退出0 | M5 1fce257f41 / 106SHA一致；独立复审待G3 | master G1；当前真实B01绑定，JS lock/SDK/shared/router/deploy唯一owner |
| G2 管理端 | eef831aa-dba4-4af2-a78b-541bc9e0a353 | 18275已退出0；当前idle | M5测试0c3caa184；三报告43SHA一致；整体58/59，字体失败 | master G2；当前自身过期空数组守卫测试修正，G1只读 |
| G3 独立复审与验收 | eef831aa-dba4-4af2-a78b-541bc9e0a352 | 67076续接；原18655已退出1 | M5守卫limited/72SHA一致；自身harness修正与固定G2变化复审 | master review/tests/evidence；产品只读，复审占前端3会话上限 |

前端固定会话上限 3，包含 G3；不会创建第四个前端会话。当前实际一个。非前端 Grok 开发上限 20，审阅按主线规则；全局 Rust 构建 10，每任务最多一个 Rust builder/一个 PG。Node 构建全局 2、浏览器全局 2，任务各最多一个；Node4GiB/1CPU。Node/pnpm/browser/font 精确版本等待 G1 实测锁定，不猜测填写。

基线：2069a8ae 前端规范入 Git；G1 原产品基线 cb5c93cf736577637f99cda4f132f34b1fb44299。G1 总卡切换提示已写入原 prompt；其 CLI 仍 live，不启动同 UUID 第二进程，当前基础 milestone 到交付边界后原会话承接完整 G1 范围。G2/G3 不伪造为已运行。

## TODO 与分层验收

- [ ] M0：完整总卡接管，固定三角色/范围/资源，G1 当前基础工作收集交付。
- [ ] M1：公共基座、字体/Token、组件、SDK、Shell/身份、管理基础接口固定并独立复审。
- [ ] M2：真实接入→构建→S9/S10→发布→Agent 问答→关闭纵切。
- [ ] M3：全部 50 页面、B01–B12 与 C01–C20 完整覆盖。
- [ ] M4：A01–A20、真实十万行 Web 旅程、部署及适用发行门槛验收。

UI 开发、实际 API 绑定、完整旅程、发行均未验收。28 卡参考开发 0/28；不并入主线后端 81 子项分母。未实现身份/发布/harness/释放只记录依赖，不用 mock/本地鉴权/伪 receipt 替代完成。所有固定 source、独立 review、证据入口和原始退出在交付后写入此台账。主 Spec/完整 Plan 包括 SP14 的标准不变。

最新统一观测：G1 handle60396 已退出0，但尚缺完整最终报告；原 UUID 恢复时先收集已启动的 Linux/浏览器 proof，不复制资源。固定三角色不变。主线新增独立任务不占前端三角色，会话复用约束仅适用于前端。统一等待已改18分钟。

最新18分钟统一观测：G1 handle44149 terminal0，尚缺完整报告；Node verify2 已启动但未核验最终结果。恢复原a351收集既有proof，不重复字体/锁构建。G2/G3仍预留未启动，验收计数不变。

G1 handle37146 terminal0；unit6/types/lint/contrast/生产build/Storybook已报告通过但尚未完整核验，browser库安装受APT降权限制失败。原a351收集browser-only重试实际退出，不重复绿测试。允许固定部分milestone提交并明确browser WITHHELD，未通过UI验收；shared组件/SDK/ModuleRegistration固定handoff后G2开始。G3只用预留a352。

固定基础切片：2c032402ee3b58801f30dbbcefddee8733e383b1；三报告已读，94 SHA条目一致。Node24.21.0/pnpm12.10.1/Chromium153.0.8010.12 build1243，固定Nodeimage481d53bf…，单位6/TS/lint/contrast/build/Storybook0；浏览器未打开、截图0，FE-01未完成。G3 a352正式派发固定源独立浏览器/设计复审，G1原a351继续总卡公共组件/SDK/ModuleRegistration/Shell；G2a353尚等待公共接口固定。前端最多3固定会话，当前将运行G1/G3两条，开发/live/旅程/发行均未验收。


2026-10-08：G1 M1 e61c0fd98b6c8b09c9c8e2db3a441aafaf952909、M2 a08b35769 已固定交付，三报告已读（22/28单位测试、类型/lint/build0；B01–B12仍unbound、live/journey/release false）。G3基础2c032402的APPROVE_LIMITED报告已读，实际20浏览器测试/19截图；哈希待协调核验，不勾选FE-01或M1。G3要求G1先补三级Token并修减少动效测试，原a351续接，G2a353因此暂未启动。G3a352仍唯一独立前端复审会话。

运行句柄登记（一次18分钟等待后统一观测）：a358=41077、a362=54243、a364=85335（既有live保留）；a351=18247（前端G1原会话）；a360=24132、a363=86745、a361=17878（原复审会话收集/裁决）；a365=86391（新迁移修复）；a366=9979（新发布复审）。前端G2a353尚未启动，G3a352上一CLI已退出。无需同UUID另启进程。

2026-10-08 本轮18分钟一次观测完成：a358/a364/a351/a360/a363/a361退出0；a362=54243、a365=86391、a366=9979仍live，未反复观测。TLS/规则/建议独立报告现已交付，尚待全报告与SHA核验，不更新已验收数。a361明确WITHHOLD E51，已新派a368=76207修复真实共同静止切点，不减验收要求；a367=17572固定6b3a92c2审核台账独立复审。G1三级Token修复866cd4e2b完整三报告已读，故固定G2a353=2137正式启动管理总卡，G3原a352=10589复审固定接口/用户路由/Token。G1当前退出无重复CLI，前端固定会话总数仍3。下一观测仍一次18分钟，实际已交付与验收计数分开。

基础G3a352 184 SHA条目、G1Token M3 20 SHA条目核验0坏/0缺；三报告完整已读。基础固定切片独立APPROVE_LIMITED成立，M1/M2用户及新版Token仍待G3固定复审；M0–M4/A01–20/FE-01不勾选。

2026-10-08 一次18分钟统一观测：a362/a365/a366/a352退出0；a368=76207/a367=17572/G2a353=2137仍live。Builder be9acbc23完整三报告/前置FREEZE已读：新pack-combined经一次核心reseal在同两角色Pack保留party/account描述，9+12与6+21测试/Clippy0，存储100k真实模型证据未变，无新模型/PG；E47未验收，独立原完整范围复审a369=16483已派。a365缺最终包，原UUID=17316收集既有真实proof，不复制资源。G3 M1-M3完整报告已读，固定路由 limitedapproved，但作者motiontest打不到实际节点/98普通中文字落入大fallback，G1原a351=44027聚焦修复不改G2。a366发布复审完整包已存在待全读/哈希，E53不先勾选。下一UUIDa370尚未使用。计数73/86、历史98、首审88、返工14、作者94/集成0/4发行0/6保持；无父项完成。
统一观测：a367/a353/a365终止0，a368=76207、a369=16483、G1a351=44027、a370=60092仍live。a365缺最终packet，原UUID27577收集已启动proof，不复制资源。新a371=65332审计WAL+真实mTLS PG ingest开发。G3原a352=88200固定复审G2三里程碑；G2原a353当前已退出。实际前端仍固定3UUID（两个live，一条已退出），不会另开。nextUUIDa372未使用。
G2完整M1/M2/M3三报告已读，source f67a87b0a/7151fcbb7/d416874ee；27/23/24 SHA核验0坏/0缺，最终类型/lint/52单位/build0，但A03–A43管理页未挂production、build未包含这些页、B01–B12 unbound、browser未执行，故不接受管理功能/live/总卡A01–A20。G3下一必要固定component复审不将独立reviewharness当publicproduction旅程。

2026-10-08 句柄与证据续接：a365=27577、a370=60092、a371=65332、G3a352=88200上次统一观测live保留；G1a351=16348（原会话续接总卡路由/组件/部署及身份schema绑定）；新备份复审a372=33067、新真实selfbuildAPI开发a373=42599。a368/a369旧句柄终止0已收包，不重启。前端G2a353当前终止0保留原UUID，无第四会话；G3当前live不重复启动，M4-font-motion固定082af80b3完整三报告已读61 SHA0坏/0缺，作者浏览器焦点变化/376字体子集有实际截图但仍待G3变化复审。G2完整页生产未挂/无binding/live不计完结。所有新task必须真实依赖和固定原scope证据，nextUUIDa374未用；下一轮仍单次18分钟统一观测。

2026-10-08 一次18min统一观测：G3a35288200/G1a35116348终止0；a36527577/a37060092/a37165332/a37233067/a37342599仍live，未再次轮询。G3 G2-admin完整三报告已读、83 SHA0坏/0缺，limited仅注册/未挂/抽查未接入组件；必改向导秘密输入、占位详情导航、空候选审核表单交回原G2a3533011；共享disabled理由/故事glob归原G1a35174888，先收集已启动M5真实退出再修。G3原a35252828继续固定M4聚焦复审。前端最多3固定UUID，当前3CLI同时运行，Node2/browser2全球cap仍必须排队，不能第四CLI。A16术语规范行B04→B05修正与真实API规范一致，源hashbaseline需后续规范记录而非假设原hash没变。主线开发/集成/发行计数不因UI未接入或schema合同提交改变。nextUUIDa375未用。

2026-10-08 G1M5三报告已读，但SHA256SUMS空且源码未固定，不能验收；原45339先收证/补固定commit，M5G2旧2emptyregistry测试未通过被作者跳过，须G2自身修订真实guard测试后在组合树验证，不能当整套测试绿。G2M4e090三报告已读24hash0，针对三处具体G3发现已修，原84790下一步等G1固定接口后改自己的旧测试。G3原15136独立复审固定切片，不第四会话。B01 backend35bd实际作者testissuer服务已交付，G1下一阶段可实际登录绑定；仅schema_frozen不是live。

2026-10-08 当前证据：a376三报告146SHA一致，但宽化search_path被错误接受，新增a378 handle44280按原契约修复，不新增计数。G2固定0c3caa184三报告43SHA一致，真实41路由守卫测试通过；整套58/59，G1字体清单多出修/订两字仍失败。G3 M5三报告72SHA一致，ENOENT缺tools archive及Router fetch符号扫描属于自身harness问题；原会话67076纠正并复审固定G2，不屏蔽字体失败。G1仍42805，下一指导已追加原卡，禁止并发启动同UUID。M0–M4及live旅程未签。

当前覆盖旧句柄：G1原a35195178、G2原a35352996、G3原a35292248启动工具确认running。G1真实可配置B01部署adapter/字体与组合suite、G2消费固定97909a4b7并完整管理接口路径、G3独立固定M6真实IdP delta。M6三报告80SHA一致、G3M5-G2三报告69SHA一致；font失败保留，真实客户IdP/完整M0–M4/A01–20不勾选。

最新统一观测：G1a35195178/G3a35292248confirmedlive，G2a35352996terminal0，M6-fixture-guard新包尚待完整核验（自述62/63，font失败），总卡管理能力仍未完成；等待真实producer，不能编造B04完成。

覆盖当前句柄：G1原a35126083/G2原a3537769/G3原a35262155工具确认running。固定9c75d686b字体/同源B01适配已作者交付，改pin后Vitest EROFS仍需可写归档必要组合测试；G3只针对fixed差异复审真实TLS与字体，G2继续真实管理闭环不把空页面计完成。

最新G1原a35150656续接工具confirmedrunning；G2a3537769/G3a35262155刚统一confirmedlive。M8 fixed96489b1c作者称63全suite+adapter5通过，完整证据待协调读取，不能提前签总卡。

M8 fixed96489b1c三报告完整读取/23SHA一致；Linux私有可写归档63组合test包含G2+adapter5通过，尚待固定G3独立复审；null deployment示例/API/ws unbound不签A20/FE/live。B01后端a6f92原E54已独立验收，客户IdP仍pending；可供真实前端/自构建服务稳定绑定。

当前覆盖旧句柄：G1原a35139862/G2原a35318470/G3原a35246149启动confirmedrunning。M9五角色已作者交付待完整读hash，G2adapter lint4归G1修，G3fixed新M8/M9 delta复审；真实B04尚待a373完整，禁止mock完成。

当前G1a35174689/G2a35371120启动confirmedrunning，G3a35246149统一confirmedlive。M10作者67suite/lint0待完整证据核验；B02/B04/B07等待固定SDK与真实producer，mock/schema不作完成。新stack395只写后端deploy/product-stack，G1web部署所有权不变。

当前三固定会话实际启动：G1a35123892/G2a35320565/G3a35273812。M9作者65SHA0坏0缺；M11prepareCall/no fetch仍不完成真实client，G1推进真实HTTPtransport，G2绑定可用fixed接口，G3必要delta独立复审。Agent入口a397唯一后端路径不改FE；不将其已派发当旅程完成。

最新统一观测G1a35123892/G2a35320565/G3a35273812全部terminal1，实证402 Grok Build usage balance exhausted；当前前端liveCLI0而非3。所有原UUID保留，额度恢复再续，后台phase待收不当完成；不新增第四会话。

用户确认额度恢复后，固定G1a35191029/G2a35397842/G3a35282686实际续接工具confirmedrunning，先收已有proof资源，不第四会话；原402历史保留，完整frontendTODO仍未签。

当前G1a35191029统一confirmedlive；G2a3533033/G3a3526454原会话续接confirmedrunning，最多三fixedUUID。来源接口B02和B04仍待fixed真实实现，source/MCP编译失败不标完成；backendpackcompat399独占后端模块不改FE。

当前原G1a35183401/G2a35387867/G3a35288570续接工具confirmedrunning，仍只有三fixedUUID。402后端binding唯一controlidentity writer，前端/其他backend只读fixed归档；真实HTTP与生产者到达后接闭环，schemaonly不计完成。

当前G1原83401统一confirmedlive，G2原94804/G3原11230新续接confirmedrunning；实际B04/发布未绑定不算完成。真实docs主链要求由403编排与现有373/392/397实现，FE保持三会话与fixedcontract消费。

当前fixedG1a35126801/G2a35385668/G3a35211844启动confirmedrunning，M13只实际B04list/get两个GET，剩余13动作仍未真实验收，完整总卡/OC主链不能签。读取对应docs/Plan再推进，每个producer/interface固定后真实绑定。
