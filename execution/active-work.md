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

2026-10-08 Codex接受原E54-D1/1真实OIDC管理身份组件：a375 closure三报告完整读取，correction96+closure7SHA0坏0缺，原68case129SHA已核验，diff逐AC复用证明+独立signedHTTPS split-audience六腿替换唯一失效旧结果，固定a6f92da8，不重跑无关68campaign。原WITHHOLD不改，客户IdP不claim/fullSP02不签。局部冻结77→78/90=86.67%；历史102→103，首次独立92→93，E54原独立WITHHOLD后修复计唯一返工15→16/93=17.20%。作者inventory待总对账原95不推测。集成0/4、发行0/6、87父项16阶段/X门槛不变。
- [x] E54-D1/1：真实OIDC code/PKCE/JWKS/RS256→PG durable管理cookie/bootstrap/current/CSRF/项目权限→logout/restart撤销（原child全scope组合独立验收）。

最新一次18min15句柄：a38494406/a38559533/a38622914/a38865101/a38926582/G1a35150656/G2a3537769/G3a35262155terminal0；a39051488/a37393577/a38342717/a38762843/a38156762/a39124465/a39230009共7confirmedlive。续原7终止会话收实际builder资源完成卡，新a393反馈固定独立复审替代已交a385，不凭退出码签收。nextUUIDa394。

补位实际启动confirmedrunning：a38459102/a3865084/a3881853/a38979702/G1a35139862/G2a35318470/G3a35246149/a39359985；连同7统一confirmedlive共15，Rust10/task1 Node2/browser2保持。新作者feedback包和FE角色包待完整证据核验不涨验收。当前78/90历史103/首次93/返工16，原全量集成发行未过。

单次18min统一15：a38762843/a38156762/a3881853/a38979702/G1a35139862/G2a35318470terminal0；9confirmedlive保持：a39051488/a37393577/a38342717/a38459102/a3865084/a39124465/a39230009/G3a35246149/a39359985。a387原真实localprocess闭环包待独立394验证资源真撤销，a381完整review待全读hash，benchmark389false不可验收。续原388/389/G1/G2，新增394session独立review和395完整stack开发，nextUUIDa396。

补位工具确认running：a38895212/a3894246/G1a35174689/G2a35371120/a39445515/a39518259，加9本轮confirmedlive共15。所有原资源优先收取不复制；当前78/90未因authorclosed/benchfalse涨数。frontend G3原46149live保留，三固定UUID不变。

一次18min统一15：a39051488/a39124465/a39359985/a38895212/G1a35174689/G2a35371120/G3a35246149terminal0；8confirmedlive a37393577/a38342717/a38459102/a3865084/a39230009/a3894246/a39445515/a39518259。新396jobs独立review、397真实Agent问答闭环、398当前权限artifactdownload；原MCP与前端三续接，不复启已交反馈/备份/jobs作者。独占新tools/agent-entry与artifact-control-api不冲现有写者，nextUUIDa399。

补位实际工具confirmedrunning：a38816513/G1a35123892/G2a35320565/G3a35273812/a39663228/a3971792/a39858504，加8confirmedlive共15，Rust10/task1 Node2/browser2仍排队不超限。feedback作者137SHA/前端M9作者65SHA0坏已核验但未签新增能力；其独立包需完整读才验收。当前78/90/103/93/16保持。

2026-10-08 Codex完整读取a391三报告并核验289SHA0坏0缺，按原BEFOREcode E51 actual100k frozenfixture范围接受固定95c33fca的完整逻辑PGbackup/hashmanifest/checkpoints→freshrestore typedgold/RPO窗口/RTO记录/readiness。独立actual普通sequence cachednextval/setval55P03、OWNED BY同值不改sequence state、完整catalog/views/comments snapshot/newobjects排除、corruptbeforeCREATEDB拒绝及freshrestore0证明；无schema/tablefilter、不用身份sequence范围替换原fixture。身份sequence添加时0A000/refuse为明确未支持能力，任意PG/全状态PITR/Git/object/Secret/production仍未验收；真实dumpfixture superuser不是生产最小权限验收。E51原withhold→repair记一个唯一返工，78→79/90=87.78%局部；历史103→104，首次独立93→94，返工16→17/94=18.09%；作者待inventory对账，集成0/4发行0/6不变。
- [x] E51-D1/1：原冻结十万行PG17私有operator一致逻辑backup→hash/checkpoints→fresh restore全typedgold与普通sequence/cat验证（a391固定独立验收，父项不签）。

反馈队列a393全三报告已读取122SHA0坏0缺，固定3be6264f独立APPROVE_LIMITED，真实HTTPSmTLS/PGreceipt与review读取/重启重放/权限隔离证明；作者a385137SHA0坏已核验。新卡未进入旧90冻结分母，能力独立证明记录但不擅自扩大计数，等全新增AC冻结inventory统一对账。

最新一次18min统一15句柄全terminal：13条exit1确认402 Grok Build usage balance exhausted（a373/383/384/386/392/394/395/351/353/352/396/397/398）；a3894246/a38816513terminal0但可能仍有后台builder资源/未交最终卡，不称资源全stop。当前liveCLI=0。禁止quota未恢复重复launch；恢复后优先原a373实际100kbuilder/源383/secret384/Agent397/stack395以及三FE会话先收已有phase/owned资源再继续，不能复制现有builder/PG。台账保存所有UUID/handle和原输出日志，nexta399未用。

2026-10-08额度中断后的只读证据推进：a381完整三报告读取，独立MANIFEST205SHA0坏0缺，原E57 fixed576e4764independent+9authorcases/Clippy0；作者a374仍缺必需SHA256SUMS，完整证据条件缺失保持E57未验收，不把reviewlimited绕过交付规则。a389实际报告loopfalse：q-entry-count config校验前失败，knowledge slug/64policy digest/shortlocalbearer契约不一致；真实model fresh sockets opened与缓存hitnot_opened有区分，costunknown不能发明金额；七wholeproductnotrun不签集成。已将具体恢复任务写原卡，禁止手写Pack/改gold/关生产bearer要求。Grok13个402终止为首次连续外阻后第二turn，本turn有证据/卡片/台账进展，goal不标blocked，无码执行授权替代。当前CLI0，额度恢复需从原会话收已有后台phase资源；不反复试402，不假15live。

用户明确恢复额度。恢复检查a37318468已观测terminal1（该次发生于恢复消息前，402日志保留）；当前无同UUIDliveCLI。原15未完成会话全部按原卡续接，各先收自己已有后台phase资源，不复制builder/PG。实际handle启动后记录；nextUUIDa399未用，maxdev20/Rust10/Node2/browser2不变。验收79/90未变，不以quota恢复计完成。

恢复后15条原UUID实际启动工具confirmedrunning：a373=56759, a383=86855, a384=88503, a386=75598, a392=11380, a394=58015, a395=58908, a351=91029, a353=97842, a352=82686, a396=29409, a397=68103, a398=60322, a388=62533, a389=30275。无新增会话、无重复原CLI；globalRust10/Node2/browser2满时排队。一次18min等待后统一这15handle，禁止中间重复日志/资源轮询。

一次18min统一15：a38386855/a38488503/a39458015/G2a35397842/G3a35282686/a38862533/a38930275terminal0；8confirmedlive a37356759/a38675598/a39211380/a39558908/G1a35191029/a39629409/a39768103/a39860322。原383388G2G3续接、399唯一Packdialogue producerfix（两个builder模块原档消费不动其他writer）、400Secretfixedreview、374只补证据共7补位，nextUUIDa401。当前79/90计数不先变，待full新包读核验。

补位工具confirmedrunning：a38331537/a3887627/G2a3533033/G3a3526454/a37417377/a39990868/a40017820，加8刚confirmedlive共15实际线。所有既有producerconsumers必须immutablefixedarchive；399 owns仅role_pack.rs/reviewed_effective.rs两模块+ontology-dialogue，其余root/工具只读，未和373/397共享写入。global10Rust task1/Node2/browser2不变，nextone18min统一15。

用户要求Self-build全阶段尽快收敛：新增specs/selfbuild-convergence-exit-plan.md明确SB01–11完整成功/失败/审核compiler真实应用/终态一致/部署与证据作者退出条件。唯一a373完整原E56继续，不把S9awaiting/jobrunning/字段未apply当交付；CLI工具中断与delivery退回/独立review返工分开统计，避免把context续接算实现返工或修改分母。当前统计79/90不变，完整SP07仍未签。

Self-build原a373实际resume handle59356工具confirmedrunning，执行SB01–11完整退出条件。上一统一观测其他live仍a39211380/a39558908/a39768103/a39860322/a39990868/a40017820；383/386/351/396/388/353/352/374原CLIterminal0保留，其资源/证据待后续一次收，不重复launch。此次用户优先selfbuild退出标准，尚未补其他新卡/实际并发不伪称15；nexta401未用。

用户要求其他任务并行≥15。补原383/388/G1/G2/G3/396（原CLIterminal），新增401本地分析独立复审、402真实管理角色绑定读写；Selfbuild37359356及既有6live不重复启动。402独占tools/control-identity和新bindingAPI，原只读消费者固定归档不冲突。nextUUIDa403，global资源不提升，待实际句柄确认后报告并发。

补位8原/新会话启动工具confirmedrunning：a3835424/a38822685/G1a35183401/G2a35387867/G3a35288570/a39647498/a40146761/a40268266。加原a37359356及上次live a39211380/a39558908/a39768103/a39860322/a39990868/a40017820共15目标句柄，老7本轮尚未重新观测，不伪称同时15confirmedlive。补完上次被用户打断的18min剩余等待后统一15一次观测，terminal先收现有proof不重复builder。

用户强调完整主链：标准YAML→审核overlay→Git版本→发布→AgentPack必须全部真实证明；新增退出标准OC01–06（docs06§3.1、SP07W01/W05/W06、SP02W02、SP08W04）及唯一集成owner a403 tools/ontology-lifecycle。原S0–S10 E56编译Pack不能替代主链完成。每卡工作前读对应当前docs/Plan并记录requirements-map，缺接口精确交原owner、固定交付消费，不mock/手写产物/假Gate。79/90原局部计数不变；主链未签。

完整主链a403 handle10719实际启动confirmedrunning，独占新lifecycle模块，与原15目标工作线并行（不是16已重观测live的声明）；nextUUIDa404。原被打断18min累计等待593.5s，尚余486.5s后一次统一16句柄；不重启原live或重复中间观测。

补完中断累计18min统一16：39211380/39558908/40017820/39990868/3835424/38822685/G2a35387867/G3a35288570/39647498terminal0；7confirmedlive37359356/39768103/39860322/G1a35183401/40146761/40268266/40310719保留。续7原closed卡各先收资源，新增404发布独立复审，400/396已交review不重启。目标15真实句柄启动后记录；nextUUIDa405。79/90未因partial交付变，OC主链未证仍未完成。

补位实际工具confirmedrunning：38363819/3887802/39971659/39598157/39239012/G2a35394804/G3a35211230/40498310，加7本轮统一confirmedlive共15。原G1a35183401不重启；Rust10/task1 Node2/browser2维持，主链OC01–06检查持续，标准Yaml/Git/activePack不能被仅compiledPack替代。
