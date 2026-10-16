最新直接人类指定 MySQL 使用 GPT-6.1 Sol / medium。此前Luna仅做现况预检，不是已完成实现；root确认其安全交接/写权释放后接续此同一原完整任务，不重复源owner。精确handoff与已跑检查见CONTROL/JSON最新mysql_rust_runtime_dispatch。

# MySQL 原Plan Rust产品运行闭环，唯一工程师

最新人类：先收敛MySQL再恢复任何其他源；按原Plan，允许的Python不为语言统一而重写。实际原Plan §0实现边界（specs/open-ontology-v0.1-plan.md）要求第一方产品与工程运行器Rust；Spec末尾仅明确文档检查器可Python、非产品/测试运行器。现有MySQL Rust C1/Loader/API/Pack/Gateway/CLI真实证据有效，但 whole.py/follow.py 持续维护/发布跟随尚是Python，不能算Rust产品运行出口完成。不要全面移植验收工具或复制通用recipe框架。

## 输入与最小修改范围

读INDEX/GOAL/PRODUCT-LOOPS/CONTROL/TODO/UPDATE-PROTOCOL、Ponytail full、docs/02、03、04、06、07、17，以及原Spec/主Plan对应SP03-W02/DBX与原MySQL完整卡MYSQL-SOL-COMPLETE.md、MYSQL-RESUME-57-COMPLETE-20261016.md。先理解接口和真实退出再改代码。

固定参考：execution/reviews/native-luna-full-source-20261015/mysql-family-complete-fixture-reference-intake.json；原完整包 /tmp/oo-mysql-connection-whole/sol-20261016/fixed-reference-complete-mysql-family-v2.json；REPORT/HANDOFF/OPERATIONS与原raw故障。实际5.7.44-log/8.4.10、C1 ROW/FULL/GTID、五表43列/110019原行、当前g4/GTID93与g7/39；旧49pins、whole run a876396217504f558f8f2f9847f143de；合法S5/两个当前Pack及固定Rust ELFs全部可复用，身份/字节先核实。旧Pyserve PID74664/74665、期限2026-10-16T20:25UTC，状态必须重核。原固定源码6042+911392259、Py6cc2fe3f4均不得改写。客户RDS只允许此前有界只读，所有IUD仅自有fixture。

唯一写权：tools/source-onboarding 新Rust Cargo.toml/Cargo.lock/src/**/tests/**；tools/mysql-preview 的现有Rust src/**/tests/**、仅确需的Cargo.toml/Cargo.lock；新增RUST-RUNTIME-REPORT/FINDINGS/HANDOFF/OPERATIONS.md。优先复用既有Rust crate/SDK/二进制/配置，不新起架构、DSL、服务或公共API。不得改root Cargo.lock/manifest、旧Python/原报告/原recipe/pins/证据/其他源；其他共享文件确需修改先向root说明精确原因/路径。root拥有全部治理、registry和远端。Doris已获hold，仅安全收取；其pipeline/Gateway/overlay修改不采入MySQL固定基线。

## 完整能力与退出（同一长跑任务）

1. 先Linux Rust1.94 locked最小编译/相关测试。用已实现Rust服务/工程运行器完成正常六字段连接输入（type/host/port/database/user/secret_ref）与明确安装的私有operator配置；普通用户不传recipe或已完成receipt。Rust入口驱动实际C1 cold-start/admission/worker/Loader与可配置持续调度，并在新代真实触发S0-S10/合法S5复用、维护标准YAML/Ontology、Pack、正式fixture发布和RO Gateway。不得仅以Rust命令再启动Python业务脚本冒充迁移；不把source/admin Secret给数据面Gateway或普通OO客户端，管理CLI按docs17调用认证Control API。
2. 收敛仅必要Python产品运行依赖：旧whole.py/follow.py及其调用链里确实承担产品/工程运行职责的部分。文档校验、外部测试资料及已有历史证据保留；不为“纯Rust文件比例”泛化重写。固定Rust流程不依赖Python解释器处理接入/调度/语义维护/发布。遇接口已实现优先直接复用，不能另造宽松信任/审核/发布路径。
3. 不与旧serve形成重复writer。先安全只读准备/验证，再记录旧Pyserve/native child状态、用明确可恢复的正常停止/控制操作完成单一owner切换；保留回退所需旧固定制品，绝不手改task/epoch/ACK/receipt。保持实际source/binding/config身份并复用合法S5，避免重建环境导致重复模型调用。无安全切换接口时先报真实边界，不能任意kill或伪造状态。
4. 新Rust正常入口与持续运行实证5.7及8.4原完整退出：CDC-first、可配置周期、真实IUD/删除、quiet不增代、崩溃恢复/CAS后ACK；全选中数据all-cell typed0、decimal/NULL/binlog/zero-date/signed TIME语义正确。至少以新Rust运行链实际完成后续自动维护/发布/RO更新，不能只沿用旧Py证明。保持角色/审核/版本/fence/只读副本/replay、未知时间诚实，原取消/拒绝/审计不减。
5. 先全无模型预检；再按原要求验证当前Pack被独立Agent经真实Rust OOCLI随机多轮消费，facts逐一对源gold/实际CLI cells/原QID admit+complete WAL。严格同source/Pack/gold的已正确证据可复用；不能继承不同Pack/代次，也不得重抽避错。产品GPT-6.1 Sol low同URL/key现已在Oracle真实通过；Grok4.7仅既有授权fallback。无必要新S5/探测/重复已通过付费QA。
6. 固定源commit/源码archive/Linux ELF/配置和全部raw exit/失败/SHA/模型usage、实际资源与截止，提供唯一Rust冷启动/重入/长serve命令及三报告。明确原Python参考与新Rust结果、fixture与客户/生产门槛。新Rust流程应能被后续源按固定接口复用，但本次只实现MySQL所需，不提前实现九源。交root做正式入口/registry切换与决定性验收，未全出不宣布完成、不短交接，无独立组件review；返工只处理真实阻断。

单独私有输出目录，所有秘密0600引用不打印；git用 /tmp/oo-dl01-wave1-git.lock + private index/精确owned提交，保留foreign stage/deletions，不add-A/reset/stash/push。构建无固定CPU/jobs上限，按实际资源复用现有缓存与制品，不重编未变库。普通clock等待600秒（单调用≤60秒分段），真实退出立即收取。唯一模型gpt-6.1-sol/medium，不子派；持续长跑到完整可验收或有不可自行解除的真实外部门槛，原目标/分母不变。
