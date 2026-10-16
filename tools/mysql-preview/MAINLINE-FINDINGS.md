# MySQL MAINLINE FINDINGS

固定证据根：`/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1`。本报告记录作者验证，不替代 root 独立接收、正式 signoff、生产验收或全 Goal 退出；其他源保持暂停，正式源计数不晋级。


## 已修正且保留失败

1. 原固定架构规则漏声明实际 gateway→builder 纯编译边。保留0b原始exit1，独立 ad28d 修复及禁止源驱动/管线转接测试，未放宽安全规则。
2. 首个二进制 prepared Date 零值解码与文本规范值不一致，真实 cold baseline 失败。正式库统一 snapshot_cell 规范化，二进制/零日期同值回归后最终 f9 冷 baseline/全型别通过。失败 ELF/exit 保留。
3. 同事务同(table,key)多事件先前选首值。真实双 UPDATE 暴露 typed difference；按 FULL 行像最终事件折叠修复，单测覆盖 insert→update、多 update、delete→reinsert/最终 delete。最终新绑定 g2 实际双 UPDATE typed0。
4. 原持续链的 native-put 仅更新 schedule，已有 loader_overrides 仍固定 c0f29a…/bba2f9…旧 producer，真实 g7/g11 又导入首值并被全量 oracle 拒绝。保留 process_attempts ELF SHA/actual exit2；官方 native-loader-put 仅换 producer f9，loader/registration/args 不变，后续 owned UPDATE 合法恢复 g8/g12 typed0，最终 DELETE 后 g9/g13 typed0。不改目标或回退位点。
5. 新 evidence parent 0755 被安全检查拒绝，运行器先创建0700私有父目录修复；未放宽 private()。
6. 新发布链依赖旧 API 缺 gate replay test_admissible，合法 replay 误拒。仅新 scope 接 root 已固定73e/3b4共享依赖，不重写共享模块；旧 API 保留原依赖身份。
7. 新 scope 与原源共用目标 DB 时 table_freshness 的既有(schema,table)键竞争，不能拼旧发布。保留失败 scope/bindings，官方禁用后用 template0 新空库与同库 jobs/target 官方协议重建真正 baseline。首次派生bootstrap/错误 target key/loader_hold 缺失/跨库 lease/新库 exact advisory-lock EXECUTE 前置失败均留；既有固定 jobs deploy +0000–4 与 pipeline install 最终0，不宣称通用跨DB产品安装器已签收。
8. 新 fixture 缺 schema USAGE、NOLOGIN role NOINHERIT 属性与原配方不齐；仅补该新 project 精确前置，保留 before/after，不授 PUBLIC/ALL/table-wide SELECT，不改原角色属性。
9. 已发布待 GW 的目录原先强制要求第7个 gateway-ready pin，形成先建后验环。b102 允许6个必需 release/source pins、可选第7个GW pin；逐SHA/拒未知/缺pin/提供GW仍全部校验。无GW pin不代表ready，后续真实目标身份/OID/typmod/GW/RO/CLI仍必须通过。
10. 多周期运行器固定读安装 seed，未消费自己原子写入的最新 ready 指针，g3/原g8/g12残留上一 shadow 列授权，mapping-reader 精确集合门禁正确拒绝。b60 修复优先私有当前指针，校验ready/RO/role、native-input同source集合/generation/Pack/私有根；坏指针拒绝，无指针才安装seed。仍逐表官方 target-close，未清 ACL 表。四 cut 合法6pin重入0，随后下一真实删除周期四链全部自动维护成功。

11. 固定归档夹带唯一非Git单测生成物 tools/mysql-preview/cursor.json（219字节、测试SID01010101…、ack/applied=false）。固定preview测试ELF在新空cwd单测逐字节复现SHA ddcb8996…fe8b8b，真实source UUID与该虚构SID不同；无build.rs/include宏输入，运行cursor只读显式from/out目录，非crate cwd。原archive/ELF保持，生成物独立provenance补充，不把它称Git源码。

## 保留的产品限制

新初始 baseline 若源提交时间未知，本轮新 g1 五表 source_committed_at=NULL，Gateway freshness_row 无法形成可信 freshness，CLI preflight拒绝（旧afff与新520均如此）。元数据/映射/GTID receipts存在并不等于 freshness 可投影。没有伪造当前时间、heartbeat 或忽略 STALE_DATA；最终真实5表 UPDATE 产生 binlog commit time 后才ready，最终QA绑定g4/GTID100或46。任意静态客户库首次即正常全链尚未签收，需后续单列产品/规范收敛。

其余原始失败包括 empty-confirm409、旧builder MODEL_NOT_CALLED、identity/bootstrap配置、singleton install_drift、共享库freshness竞争、immutable_output_drift、新GW旧seed relation缺失、mapping role不符、工具组件未装及辅助诊断命令失败；均保留原日志，不抹为0。一次不受管follow进程退出无法取得，mysql57-rust-follow-g4-v2.unknown-exit.json明确UNKNOWN。终态旧serve取到2/143分别记实，长serve仍运行的exit为pending，非0。

## 安全与成本边界

Gateway仅自身运行目录和只读固定ELF目录，不挂 source/admin/model/socket。final-runtime-observation-v1 有四GW实际inspect和source/admin Secret不可读探针；final-physical-quiet-v2 有真实TLS/physical standby/RO、READ WRITE SQLSTATE25006拒绝、CAS后ACK和quiet apply=false/ack=false。控制容器为受信fixture admin，私有凭据引用0600，未输出值。源只有ownedfixture DML；客户源只读边界未改。

原49pins/旧不可变完整包/旧Python报告与原成功cut不改。原QA只保留历史，未跨新身份/gold继承；本轮55 response/137055 tokens完整公开，工程师总usage未知。正式全源/生产/parent计数没有晋级。
