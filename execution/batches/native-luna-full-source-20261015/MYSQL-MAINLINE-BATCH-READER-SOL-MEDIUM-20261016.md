# MySQL批量源读取器进入正式主线：完整能力任务

唯一工程师 `/root/mysql_rust_runtime_sol_medium`，GPT-6.1 Sol medium。用户最新明确：MySQL源端批量读取器放进oo-connectors，整条链路在主线通。这是暂停总Goal之后明确授权的MySQL专项；总Goal/其他九源保持paused/held，Doris已安全终态释放writer。Ponytail5.3 full、最小完整修改，复用已有实现，不重写框架，不派组件review或子工程师；同一长跑到全部退出才报完成。

## 输入/接口与最小范围

先读INDEX/GOAL/CONTROL/PRODUCT-LOOPS/TODO/UPDATE-PROTOCOL、原docs03§4/§5.1、04、06、07、17、database-support02/03/04、Spec/Plan的SP03-W02/DBX原全部要求。原Connector契约的有界异步Arrow批流、只读/TLS、预算/取消、型别、源和目标凭据边界保持。当前oo-connectors只有PgConnector；MySQL实际在tools/mysql-preview/source.rs/map.rs/values.rs/version.rs、C1 snapshot_rows等，source.rs还耦合oo_pipeline的COPY/hash/QualityReject，不能整文件倒搬造成循环依赖。

固定输入：root已验收 `execution/reviews/native-luna-full-source-20261015/mysql-rust-native-complete-intake.json`，e3运行链/ac633默认入口、两个原不可变完整包、5.7.44-log/8.4.10，各110019行5表43列typed0；合法S5/当前g5/94和g8/40的Pack/随机八轮原QID WAL可按严格身份复用。现有两个Rust serve/API/GW/DB/物理standby与私有operator均保留。09:24UTC root独立续签client receipt位于 `/tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/root-client-renewal-20261016T092455Z/receipt.json`，SHA6df6adcd12b84bd2a37b3a9ac01aad11fc70c1c758a1e858603839c666779e78；是显式新token，非默认替换，约10:24UTC到期。不要用旧unsafe root刷新脚本（旧GW已停/旧位置93/39）。需要时仅控制面官方issuer签新私有文件，Gateway绝不持admin/source/model/socket。

独占写权：crates/oo-connectors/Cargo.toml、src/mysql/**及lib.rs/必要现有类型辅助和tests/README；tools/mysql-preview/src/**与tests/**、该独立workspace Cargo.toml/Cargo.lock；根Cargo.lock（仅必要锁定新增MySQL驱动依赖）；根Cargo.toml仅确需manifest接线；tools/source-onboarding/src/**仅本实际主线消费入口必要窄接线；新增 `tools/mysql-preview/MAINLINE-*`报告。其他共享文件先给root精确因果与路径再扩写。不得碰其他源WIP、已安装迁移、vendor TLS patch/安全门禁、原49pins/旧收据/旧报告。当前根Cargo/oo-connectors/MySQL目录已核无源码WIP；Doris reader未提交WIP保持。git锁/private index/精确owned提交，禁止add-A/reset/stash/push。

正式公共接口应提供仅源端配置/SecretRef/TLS、受选表/catalog/type及有界批读取；复用现有oo-core/Arrow类型，类型和能力诚实。不要求提前造全十源空trait/空registry；也不能只加名字、依赖、re-export或通过path/include回指tools实现。oo-connectors不得依赖tools/mysql-preview或oo-pipeline；Loader/COPY/目标DDL及目标管理员凭据留在管线/适配调用层。移植原映射/decoder/版本逻辑到正式crate后旧调用方改用它，避免保留两套MySQL解码/读取实现。实际C1 baseline/reconcile批读取调用链也要盘清并接入，保持原binlog CDC路由与CDC/批量类型/规范值一致，不改成C5快照产品。

## 全部退出

1. 先Linux1.94 locked最小冒烟；构建当前正式workspace/固定Git archive+本任务明确delta，不从foreign工作区拼半成品。依赖复用原mysql_async=0.36.1及必要已锁features，不换驱动/TLS栈或新增通用依赖；更新根lock后锁定构建、架构依赖门禁、相关Pg/MySQL测试真实过。不得放宽门禁/校验或只macOS跑通。
2. 真正MySQL批量读取在oo-connectors中实现且公共可用，5.7与8.4的版本语法/TLS/只读体检、catalog/type、分块有界Arrow读取/取消清理实际可证。选中表列/decimal/NULL/zero-date/signed TIME/无损精度等保留；错误CA/主机/不支持版本/越界或取消等原必要负例不删，不堆无关测试。
3. 主线消费者真实调用此正式实现，preview只留测试/薄适配，不把主线库变成工具包装；不存在循环依赖或Gateway获得源驱动/Secret。必须从主线Cargo代码构建实际使用的reader/producer并给出源码→依赖→ELF→调用证据，不能仅搬文件/改README。
4. 两真实版本完整主线联结验通：源→新核心批读取/Arrow→正式Loader/复制数据→原合法S0-S10及当前维护YAML/Ontology/Pack→fixture发布/物理RO→实际Gateway/Rust CLI。原C1更新/删除/quiet/恢复/ACK退出保持。合法同身份/同Pack/gold已有证据优先复用；必要真实新cut/新source的证据按原边界补齐，不能拼不同身份/旧发布冒成功。先无模型预检，结构搬迁不重复付费；需要新模型才按实际身份调用Sol low原URL/key（授权Grok4.7 fallback），不省真实必要退出。
5. 固定源码提交/主线归档/Linux ELF/锁文件与依赖方向、真实入口/两版raw exits、全量typed0和新核心被实际执行证明、必要成功失败与资源/期限/usage，交MAINLINE-REPORT/FINDINGS/HANDOFF/OPERATIONS及不可变完整包。明确原fixture参考和现在主线集成的区别，不宣称任意客户/生产/全部十源通过；root决定性核验与全局台账/安装/远端专有。未全出不能短交接为“完成”。

无CPU/jobs固定限制，独占源码与独立build target/cache保持；源预算不变。普通clock600秒（单调用≤60秒分段），真实退出立即通知，不短轮询。普通进展少报，真实阻断需root扩域或完整包才及时通知。

## 2026-10-16 用户结构确认

按源模块组织，不把后续二十余源堆入一个文件。当前仅实现 mysql 模块；MySQL 专有依赖经 Cargo `mysql` feature 可选启用、lib.rs 条件导出、实际消费者显式启用，PG 默认路径不拉 MySQL 驱动。最终固定归档须验证默认/启用 MySQL 两种构建与真实消费；不造其他源空模块、通用工厂或动态插件框架。

## 2026-10-16 既有精确依赖声明漏项窄修

原0b27固定基线的架构检查存在既有 oo-gateway→oo-builder 未声明边；Gateway 已调用纯 compile_reviewed_sealed_import 校验，builder/jobs 无源/控制驱动或模型runtime。root 核实际调用与 manifests 后，仅扩写 crates/oo-arch-check/src/policy.rs 的 REQUIRED_EDGES 精确声明该既有边，及必要最小现有测试；全部源驱动/凭据/管线/runtime 禁止规则保持，不抽搬编译器、不忽略失败。保留未改基线 exit1，独立窄修提交，Linux 正例/注入禁止边负例/恢复基线均须通过。此为同步已有实际依赖，不授权新增 Gateway 源访问。
