# 当前执行板（Codex维护）

更新：2026-10-08。只汇总最近一次真实句柄观测与启动；不把台账或源码锁当作运行证据。原始历史/冻结验收标准留在specs三个主台账。一次18分钟等待后统一观测，无中途重复轮询。

| 工作线 / UUID尾号 | 实际handle | 最近状态 / 固定来源 | 唯一写入边界 |
|---|---|---|---|
| E55 审计WAL/ingest a371 | 65332 | 统一观测live；未交最终包 | tools/gateway-audit-wal、tools/audit-ingest、自身证据 |
| E56 Self-build API a373 | 56260 | 原42599退出0但无报告；原会话收现有retest并接真实身份 | tools/selfbuild-control-api、自身证据 |
| E57 curated Git a374 | 36873 | 统一观测live；合同9d33bfc06，未交最终包 | tools/curated-git-apply、自身证据 |
| E54 OIDC独立复审 a375 | 89681 | 统一观测live，固定35bd6b815/合同089b | 自身证据；产品只读 |
| 同库迁移复审 a376 | terminal 0 | 三报告/146SHA一致；exact search_path契约缺陷交a378，不签修复完成 | 自身证据；产品只读 |
| 严格search_path修复 a378 | 44280 | 已启动；固定be4b59648，禁止重复启动 | tools/calendar-store、tools/enum-store、自身证据 |
| E51 全dump切点返工 a377 | 86777 | 统一观测live；旧20f065096被a372 WITHHOLD | tools/pg-backup-restore、自身证据 |
| 前端G1 a351 | 42805 | 原45339退出0，M5固定1fce257f41；下一真实B01接入 | 总卡G1；JS lock/SDK/shared/router/deploy唯一owner |
| 前端G2 a353 | terminal 0 | 固定0c3caa184，三报告/43SHA一致；58/59整体测试，字体失败归G1 | 总卡19管理features/自身tests；G1只读 |
| 前端G3 a352 | 67076 | 原18655退出1；M5三报告/72SHA一致；修自身archive与network扫描并复审固定G2 | 总卡review/tests/evidence；产品只读 |

主线开发上限20，独立复审不占主线开发槽；前端总共仅上述三个固定会话（含G3），禁止第四会话。Rust全局10/task1含host/helper/hashbuild，6GiB/1CPU；PG task1 1GiB/1CPU；Node全局2/task1 4GiB/1CPU；browser全局2/task1，实际满则排队。

## 固定来源 / 依赖

- B01：合同089b00d7f；作者真实HTTPS OIDC实现35bd6b815，完整包24hash0；a375未验收。B04可用固定作者服务做集成，不伪造已通过独立审核。
- B04：合同7bb2da3cc；a373尚未交固定实现。前端只能schema_frozen，不能live/假候选/假审核。
- Builder原E47：be9acbc23，经a369原完整范围APPROVE_LIMITED，117hash0；单一combinedPack保留两表批准body。fullSP07/auth/pub未签。
- durable审核ledger：6b3a92c2，E52已验收，但child authenticated=false、尚不Gitapply。服务需真实身份与对象/候选/编译catalog成员关系绑定。
- Git：a374待交付。a373不伪造curated已落Git；后来集成仅消费固定适配器。
- 前端M5：1fce257f41dedd1e460cb4180d5286e256caa10d，106hash0；两条G2旧空数组断言被作者跳过，整体suite不能当绿。G2更新后在固定组合树检查。
- 备份：a372原E51 WITHHOLD，持锁期间view/schema/table/sequence新增被入dump却cut_proven=true，a377修复，禁止仅改变支持范围或关所有readiness来验收。

## 分层计数 / 下一步

后端局部冻结76/90，历史验收101，首次独立组件91，唯一返工15/91；作者95。全量开发分母未冻结。完整87父项/16阶段仍未全签；M1-D17/24；集成0/4；发行0/6；X01–X05真实owner/客户范围/保护CI/runsc/UAT独立pending。企业源连接其他信息未收到，未经配置不能连接；所有实际产品试验本地PG，不使用企业只读账号做直接E2E。

下一未用UUID a383。任务退出先收已有真实资源/phase exit/完整三报告/source/SHA/cleanup，缺报告不能凭CLI0验收、不重复启动现有builder/PG。所有源码/测试/自动化/审阅由Grok4.7 xhigh执行；Codex写规范/卡片/台账、核验证据、精确Git提交。

## 最新统一观测与派发（覆盖上表旧句柄）

一次18分钟后，65332/56260/36873/89681/86777/42805/44280/67076均terminal0。CLI0不等于验收：a375明确WITHHOLD E54，aud错误绑定configured audience而非client_id；a377序列切点仍未证明，E51未完成；a373既有builder exit6，原会话继续收集失败并推进真实全程。

| 当前任务 | 已确认运行handle | 边界 |
|---|---|---|
| a379 E54 audience修复 |22340|tools/control-identity、自身证据|
| a380 E55独立复审 |72955|自身证据，固定1078d69e8只读|
| a381 E57独立复审 |93531|自身证据，固定576e476f1只读|
| a382 search_path变化复审 |64115|自身证据，固定9a7940ba5只读|
| a373 E56原会话续接 |83407|tools/selfbuild-control-api、自身证据|

G1/G2/G3此刻均terminal，未另启。G1 M6真实fixture-admin授权码登录/初始化/current/logout已有包，仍待完整读取和哈希及固定独立复审；不标客户真实IdP或完整FE。G3 M5-G2证据称font506对504失败，尚待全包核验。a371/a374/a378作者三报告已读（哈希待核验）；a375三报告已读明确WITHHOLD。此次无验收计数变化。下一步先核验未读包及所有SHA，再续固定前端三会话与备份原范围修复；不得根据本表终止状态重复启动已有资源。

2026-10-08：闭环派发规范生效。新能力作者负责从实际输入、真实依赖调用、持久状态、成功/必要失败、可复现启动到固定提交/完整证据，自述CLI0或helper/schema不计交付；独立复审仅检验固定交付。新增a383源接入catalog API、a384 Secret写入轮换、a385反馈接收至审核队列、a386完整本地分析、a387本人会话至真实S1–S11关闭、a388官方rmcp stdio至Gateway、a389十万行真实模型benchmark；原Plan对应SP03/05、SP02-W04/W06、SP09-W05、SP11-W01/02、SP13-W01、SP12。完整卡AC需作者实施前冻结，未完成不计开发，现76/90分母不因派发数量变化；全量Plan范围不缩。G1/G2/G3仅原三会话续接；备份a377原卡继续完整sequence切点与成功restore，不接受仅拒绝模式完成。主线开发cap20、Rustglobal10/task1、Node2/browser2保持，资源不足排队而非重复builder。外部门槛单列，禁止mock替代完成。

新卡a383–389及前端三续接/备份句柄启动后记录；nextUUID a390。a374作者SHA256SUMS缺失，暂不签收，a381独立复审需核对source并保留证据缺项。

## 闭环工作线实际启动

- a383: handle 73258，启动工具确认running，禁止重复启动。
- a384: handle 94406，启动工具确认running，禁止重复启动。
- a385: handle 59533，启动工具确认running，禁止重复启动。
- a386: handle 22914，启动工具确认running，禁止重复启动。
- a387: handle 19418，启动工具确认running，禁止重复启动。
- a388: handle 65101，启动工具确认running，禁止重复启动。
- a389: handle 76805，启动工具确认running，禁止重复启动。
- a351: handle 95178，启动工具确认running，禁止重复启动。
- a353: handle 52996，启动工具确认running，禁止重复启动。
- a352: handle 92248，启动工具确认running，禁止重复启动。
- a377: handle 46898，启动工具确认running，禁止重复启动。

既有a37922340/a38072955/a38193531/a38264115/a37383407保留，上次启动live。本轮11条新启动，连同既有5条待统一观测共16条（开发12、主线复审3、前端G3复审1）；实际Rust最多10、Node最多2/browser2，超出仅排队。独立复审不占主线开发20上限，前端仍恰好三个固定UUID。一次18分钟等待后统一16句柄观测，不根据数量虚签验收。已核验M6 80SHA、G3guard69、WAL23 source、OIDCreview129、backup255、searchpath99全部0坏；a374作者缺SHA留待补齐。

最新单次18min统一16句柄：a37922340/a38264115/a37383407/G2a35352996terminal0，其余12句柄confirmedlive，保持原句柄不重复启动。a379修复fixed evidence dc434a4a6，a382报告limited尚待完整核验，不增加验收；G2新版63测试62pass/font1失败、B04未接，不能完成。a373真实archive workspace补齐后仍builder101，原会话修具体失败。新增a390完整SP02-W01-b Rust100worker job运行时闭环，原a375重审固定audience correction，nextUUIDa391。

实际补位启动：a39051488、a37393577、a37567327工具均确认running；加12刚统一确认live共15实际工作线（开发11+review4），不凭未启动卡片计并发。G2此刻idle，前端仅G1/G3live仍三固定UUID。下一轮单次18min等待再统一15handle。

2026-10-08 Codex完整读取a379/a382各三报告并核验SHA：56/102条0坏0缺。a382固定9a7940ba5独立APPROVE_LIMITED，实际PG widened public/pg_temp/prefix/extraGUC拒绝且catalog不变、恢复精确配置replay，Rust1.94 focusedunit/Clippy/实际proof0；接受search_path原缺陷修复，不新增开发单元或计数，不把raw双psql/SEM父项/SP14/E51标完成。a379固定a6f92da8a0bf872f8a30efb116c2c7b14463505b真实signedIdP audience/client_id修复作者已交付，56SHA一致；E54仍待原a37567327独立复审，尚不验收。

单次18min统一15句柄：a38193531/G1a35195178/G3a35292248/a37567327terminal0；a38373258/a38719418/a37746898terminal1 Internal error。其余8confirmedlive（a38072955/a38494406/a38559533/a38622914/a38865101/a38976805/a39051488/a37393577）。8终止原会话续接只收既有资源/固定证据再推进，严禁复制builder；G2 idle原UUID消费固定9c75。下一UUID仍a391。

实际原会话补位启动确认running：a38342717/a38762843/a37730117/a38157162/G1a35126083/G2a3537769/G3a35262155/a37564094；加8刚观测live共16工作线。所有writer原路径不变；Rust10/Node2/browser2配额排队。下一轮单次18min统一16handle。

2026-10-08 Codex接受原冻结E55-D1/1：a380固定1078d69e8独立APPROVE_LIMITED三报告完整读取，108SHA0坏0缺，实际DedicatedSession准入fsync在首行前、Collector故障留存/独立reader重启、mTLS项目绑定、PG同事务event/receipt、重复与冲突、ack hash后GC、fsync/quota/私有ENOSPC无query，Rust1.94 focused+Clippy0，ownedcleanup已证。作者测试某live skip并未作为验收，独立boundary实际运行通过。只签原E55审计耐久组件，不签publicSQL/feedback/fullSP06/runsc。局部冻结76→77/90=85.56%；历史101→102，首次独立91→92，唯一返工15/92=16.30%；作者待新交付inventory总对账不推测，原记录95保留。集成0/4、发行0/6、完整87父项16阶段/X门槛不变。
- [x] E55-D1/1：真实查询准入/完成fsync WAL→真实mTLS PG耐久receipt→验证确认后GC（a380固定独立复审）。

单次18min统一16：a38072955/a38976805/a37730117/a38157162/G1a35126083/a37564094terminal0；其余10confirmedlive。a380原E55已验收；a375原E54完整组合verdict已交付待完整读取/哈希；a377真实sequence lock+restore成功新包待独立原scope审，不先验收。原a389继续现有chain/a381修自身sourceGit harness/G1推进总卡；新a391备份独立复审/a392真实发布回滚产品API，各独占路径。nextUUIDa393，开发cap20/资源Rust10/Node2/browser2不变。

实际补位已确认running：a38926582/a38156762/G1a35150656/a39124465/a39230009，加10本轮统一confirmedlive共15工作线。原closed不重复观察/启动，actualRust构建10/task1仍需各任务排队，frontend固定G1/G2/G3。当前冻结77/90，未核验新OIDC/verdict或backup不涨数。
