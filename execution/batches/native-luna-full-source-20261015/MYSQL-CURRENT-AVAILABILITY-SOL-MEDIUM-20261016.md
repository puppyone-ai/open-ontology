# MySQL 当前完整可用性：同一工程师长跑修复及验收

2026-10-16 人类明确“必须保证当前完整的可用性才能结束工作”。仅恢复 MySQL 完整能力修复/测试/必要服务操作，其他九源与广义 Goal 仍暂停。唯一工程师原 /root/mysql_rust_runtime_sol_medium（gpt-6.1-sol/medium），不派组件作者/审阅子智能体。Ponytail full、Rust主线、最小必要修改；保留失败，不用无休止试错、假时间或降低门槛换成功。

## 当前固定输入与故障

前卡 MYSQL-MAINLINE-BATCH-READER-SOL-MEDIUM-20261016.md、原 docs03/04/06/07/17、database-support02–05、SP03-W02和原始退出全部继承。源码 b60bd6fb52c7b021c655bd91ee003d88c84ac204；报告9770f4a1f，root接收a57717964与健康更正72c431ab1。输入根 /tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1，DELIVERY.json/MAINLINE-*与当前健康 execution/reviews/native-luna-full-source-20261015/mysql-mainline-current-health-20261016.json。

隔离 mysql8 g4/PID92824 已退出2：1692-native-fire.stderr 为 unexpected EOF（前一正常quiet 1690，失败证据原样保留）；其余3serve和默认5.7/8.4 CLI目前可读但不能据此全链保证。初始新g1五表 source_committed_at=NULL 导致Gateway无法投影freshness，必须能在不写源制造时间的情况下从静态库完成首次链路。客户端TTL1h，serve24h/owner锁和官方恢复保持；令牌续签须显式官方issuer，不能关闭认证或挂admin入GW。

## 唯一写域与输入边界

继承前卡 oo-connectors/mysql、preview、source-onboarding、精确arch规则/现有回归写域；此次直接授权 tools/job-worker/src 中 native-fire/连接生命周期与对应最小回归的必要Rust窄修，及 crates/oo-gateway/src/public_surface.rs 的实际freshness投影/最小现有回归（不得放宽发布/类型/OID/权限/拒绝安全门禁）。相关其他文件须先给root精确因果申请。尤其 foreign staged oo-pipeline 等WIP禁止采入，根Cargo/锁只确需依赖更新，不能重写通用recovery/plugin框架。若正确修复应在其他共享现有路径，及时报root申请，不将非阻断小质量问题扩大。

客户库不写。源DML只用已授权ownedfixture；初始静态验收不依赖任何源写入或伪造source commit time。先读真实query/executor/协议契约，未知源提交时间保留unknown/真实观测边界，source time与snapshot capture/apply time不得混为一谈。不要把真实断连只用手工重启藏掉。

## 一次交付全部退出

1. 定位unexpected EOF具体失败层（docker exec/worker连接/SQL等），保留原日志。最小修复真实短暂断连导致永久停机的问题，只有幂等/官方CAS或原task恢复允许重试；永久认证/配置/语义错误必须明报，不能无限吞错。模拟必要故障测试及真实owned环境一次故障恢复，不能产生重复apply/ACK或非法水位。
2. 两版5.7.44-log/8.4.10：正式Rust connector真实基线→Arrow/Loader→S0–S10及维护YAML/Ontology/Pack→真实发布/物理RO→Gateway/CLI。至少验证新静态baseline在没有任何源DML的前提下可以完成正常查询；源时间unknown要诚实呈现，不伪造heartbeat。复用当前schema/合法固定构建与相同身份封存品，需新scope则官方隔离前置最小复用，绝不改旧不可变收据。实际GTID/真实源全量typed oracle与快照一致性要可证。
3. 两版完整更新、删除、下一quiet、可配置周期、官方重入/异常恢复全部真跑；源真值/target逐类型0差、CAS后ACK、当前Pack与数据/源身份绑定，Gateway物理只读且Secret隔离保持。停止旧源writer后才移交/启动同binding唯一新serve。原默认两条和新验收两条要么四条全部恢复并验证，要么证明只需保留唯一每版正常链后由root明确归档额外链，不能静默丢失败。
4. 最终固定Linux Rust1.94 locked制品、相关回归/Clippy/架构方向均通过；部署后至少连续10分钟实际周期和新cut/quiet证据，涵盖一次真实断连恢复，结束前所有保留serve/CLI/token当前有效。真实随机两组各两turn/版共8turn，合法同identity/Pack/gold的历史可保留但须补每版至少一次当前真实模型→CLI→Gateway调用并严验原成功QID/WAL/事实；若新Pack/身份则原8turn重验，不能固定问题。零模型预检先行、最短Pack上下文、沿原随机选题断点，目标新产品tokens<=27411；实际usage全数披露，不能为了额度删必要事实验证。
5. 固定源commit/归档/ELF/依赖/三报告与operations/完整manifest、全部raw exit/恢复前后状态/usage/服务期限/owner进程交付，root可单命令只读核当前进程、状态、全量typed、Pack、Gateway/CLI/成功WAL。不是只“实现完成”或“启动”就报完成；root接收失败沿本卡继续到当前完整可用。root拥有全局账务、默认安装、远端同步，作者不能推送/改总Goal/签收分母。

## 资源及等待

复用固定Linux1.94/cache/ELF，独立target；无固定CPU/jobs限制但保留实际内存和源只读预算。所有现有源DB/API/standby/失败证据保留，不碰其他九源。默认clock600秒等事件，不短轮询。只在真实需扩域/关键失败/完整退出时简短报root，长跑不短交接。未全部退出不可宣布完成，独立统一复审仍按用户原十源整链时点。

## root窄域实现确认（保留原门禁）

仅MySQL mysql-gtid完整发布/逐表receipt/epoch/范围精确覆盖且真实applied_at有效时，可支持缺失source commit time：freshness端点沿既有source_time=unmeasured补明确target_applied_at观测；query沿既有hints说明未知，freshness数组不伪造as_of/lag。runtime preflight必须精确核当前向量覆盖与时间字段，不能只删长度校验。非空非法时间/负lag继续拒绝，其他源和API/core结构不变。native-fire只对明确docker exec瞬时EOF做有界重试；既有fire事务active_task_id/occurrence/pin及older queued/running/unacked门禁保持，保留每次失败和真实恢复去重证据。外域job-worker代码若无需修改保持不动。

## 既有Gateway风格lint的分层处置

9b9ecb5c8运行器Linux测试/Clippy/release实际0、Gateway71测试/release0；Gateway严格Clippy101，同原固定520源码独立重放的11条既有lint完全相同，本轮无新增。按人类“非致命小问题延后、优先完整可运行闭环”不扩域修旧风格lint；必须保留两份失败和逐项对照，不称严格Clippy通过。正式质量/发行/父项门槛仍待满足，不由当前产品可用性验证代签。
