# Doris 完整原Plan闭环：MySQL实际验收后的唯一长跑

唯一工程师 `/root/doris_complete_luna_max`，保持原 GPT-6 Luna / max，不子派。此卡覆盖旧卡历史排期/已安装迁移/共享写权，仍是同一Doris源任务，无新分母。Ponytail full，最小完整Rust改动；不因普通小问题重构，不复制通用recipe框架。root负责全局台账、安装registry、远端与最终签收。普通等待clock.sleep共600秒，单调用≤60秒，退出立即收取；不短轮询、不短交接。

## 固定输入与现况

先读INDEX/GOAL/CONTROL/PRODUCT-LOOPS/TODO/UPDATE-PROTOCOL、Ponytail和codex-grok；再读原docs03、04、06、07、17，database-support03/04/05，原Doris Spec/Plan/REQ/AT及DORIS-C4-C5-COMPLETE-SOL-20261016.md / DORIS-COMPLETE-LUNA-MAX-20261016.md的全部原退出。不得按旧快照修改目标。

MySQL已由root真实核验并安装：`execution/reviews/native-luna-full-source-20261015/mysql-rust-native-complete-intake.json`，协调commit `ac22ed665`。e3e83b399为完整C1 Rust运行链，ac6332654为默认六字段入口，Linux1.94固定ELF/归档、S0-S10/维护/发布/物理RO/CLI/四组8轮真实Agent均有固定证据。原v10b包f76e0148…3754与native补充da20ca45…f4db可复用；MySQL所有运行容器、数据、令牌、49pins、历史报告/证据不许修改。现在只有本源恢复，Oracle及其他7源保持held。

Doris原hold：`execution/reviews/native-luna-full-source-20261015/doris-mysql-priority-final-hold-intake.json`；私有HOLD-RECEIPT SHA e7948ff1cd0cdcc0cf41349047968bbd42ade6016448681ceae1c96e6e21013e。原writer已terminal释放，10bindings已官方disable，原v15 PID2331已退出；一条C5 task ffe68a40-b879-4da5-a528-9d4e3731d1c4因target_database取消拒绝1仍queued，不能称已清空或手改。source/target13待发布，旧published9；现有DB/API/GW/volumes与全部失败保留。恢复先核真实source/binding/project/target与/proc ELF，不能以manifest猜当前Gateway已装。

保留固定输入：3b4fef066公共API/0039–41；Doris修复c6d7fc0e3d7e62cde537ae5878a973962d720d91，archive389f0506937353c0f1b788f962ff514de4ebb6ca0973cebdde0e4efdcddcbbf7；0042已官方安装，原40行→41行，旧校验和/owner/ACL不变，root已核。c6只改0042/native_update/target_installer；不盲cherry-pick/reset foreign index。Doris质量Gateway候选b65e90013cc97a23f13bbdb5414c648ec9280d30/ELF520bd301…98364b原71tests及mapping-reader批准仍是可复用固定输入，实际在跑哪个必须现查。成功S5 b_bb4093b86850c9a35fbce465001b2f6d合法复用，18可见/20metadata两隐藏与unknown money保持，不新模型探测。

## 独占修改与资源

源码仅本源tools/doris-preview/**、testing/dl01-driver-preflight/doris/reader/**；新增必要Rust `tools/source-onboarding/src/doris.rs`及同crate main.rs/product.rs/io.rs/api.rs 的最小确需复用/dispatch/表级向量分支与inline tests。原MySQL分支及其测试必须保持；无必要不改Cargo。原共享pipeline/Gateway/release/worker/overlay/pg-builder固定实现优先只读复用，确有真实阻断时先给root精确路径和原因才扩写；已安装0000–0042不可修改，不新迁移/手授权限/改rootCargo。不得改共享Python、旧49pins或其他源WIP。所有构建从固定archives+明确owned delta隔离组合，不编共享工作区半成品；无固定CPU/jobs上限，按实际硬件使用独立可写target/cache。

仅恢复或自备本源具名FE、target/standby、identity/API/release/GW和私有端口，原资源交接优先复用。不向用户要数据/实例，不碰客户库写入。控制面可持admin/source Secret和Docker socket，数据面只挂必要RO运行凭据/client token，必须实际证明admin/source Secret与socket不可读；不能重复旧Follow整目录挂载错误。安装注册仅root操作，给出真正Rust安装格式与命令，不伪造旧Pythonwhole协议。

## 一次长跑的全部退出

1. 先Linux1.94 locked最小冒烟。以实际原doc C4分区刷新及C5差分恢复原完整链，保留既有真实C1不变；不全量快照替代C4、不捏造全局位点。当前已发布parent copy-forward0042实际入口验证与先前故障保留，queued目标错配通过正确官方配置/接口处理，不手改任务/epoch/ACK。
2. 正常六字段到本地Rust一键入口（私有operator由安装者预置，用户不提交recipe/receipt），接原Rust reader/producer、scheduler/admission/Loader；两个独立真实IUD批含删除和分区替换、可配置周期、quiet不增代、同任务崩溃恢复、CAS后ACK；全受选真实表列typed oracle0差异。历史IUD可在严格同身份时复用，不把不同代/源结果冒本轮。
3. 原真实S0-S10与合法S5证据绑定当前维护API，产生标准YAML/Ontology和角色Pack并实际消费C4/C5方法/分区、逐表代次、删除/一致性与真实类型语义。原Plan第一方产品/工程运行器Rust：只补本源必要持续接入/调度/维护/发布编排，不为语言统一重写外部fixture/文档校验/Agent验证器。不得Rust转调Python业务脚本。Doris逐表混合代向量不能套MySQL同代假设，也不把NULL全局位置造0。
4. 真实fixture审核、正式发布、版本/fence/角色ACL和物理RO副本replay；实际Gateway连复制数据，知识内容匹配当前真实表。发布复用必须核当前active/operation/source vector、目标published身份及实际pg_attribute OID/typmod；不得仅靠行值相等或旧成功receipt。保持必要拒绝、取消、rollback与持久WAL。
5. 先全无模型CLI/sourcegold/currentPack/type/权限预检，再两组互不重复随机语义、各2轮实际独立Agent经Rust OO CLI公共工具对话。产品GPT-6.1 Sol low原URL/key，已实通；Grok4.7仅授权fallback。不给SQL/答案/凭据，不重抽避错、不调弱validator；最终facts逐项独立源gold/实际CLI cell/原成功QID admit+complete WAL。严格同source/Pack/gold正确QA可复用，不重复S5/模型。
6. 完整正常入口与同输入重入，固定源码commit/archive/Linux ELF/依赖/配置/全部raw exits与失败/SHA/usage；三报告+操作与安装指令，原始对话、当前长期唯一writer或安全终态/期限/令牌到期均可验证。未全出wholefalse，不以新组件/局部成功短交接。root决定性核验再安装新默认与清退旧GW，统一独立审阅仍待十源完整链；正式源0/10、原Plan/父项/整版分母不变。

提交仍用fcntl /tmp/oo-dl01-wave1-git.lock + private index/精确owned路径，禁止add-A/reset/stash/push。一次任务到完整闭环或不可自行解除的真实外部门槛才报终态；普通进展无需逐段通知，关键阻断/完整包及时交root。
