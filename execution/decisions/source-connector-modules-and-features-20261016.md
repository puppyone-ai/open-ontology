# 数据源实现模块与 Cargo feature 口径

2026-10-16 用户确认：MySQL 批量源读取器进入正式 `oo-connectors`，整条产品主线实际消费；后续源按模块扩展，不堆入一个文件。本条是实现组织与构建边界，原功能、安全、捕获方法和签收条件保持。

- 每源一个模块目录，例如 `crates/oo-connectors/src/mysql/`；目录内按连接、catalog、批量读取、类型映射、解码和版本分文件。`lib.rs` 只导出入口，不集中所有源的实现。
- MySQL 专有驱动及专有依赖用可选 Cargo `mysql` feature 启用，模块条件导出；实际消费者显式启用。现有 PostgreSQL 默认构建不引入 MySQL 驱动。后续 Rust 原生源采用同样的按需依赖原则，不预造空模块或所有 feature。
- 复用已存在的驱动、类型映射和解码；真实实现归正式 crate，preview/测试工具调用正式模块。不得让正式 crate 反向依赖工具、通过 `include/path` 假搬迁，或保留两套相同行为。
- 连接器负责源端只读连接、体检、catalog、类型与有界 Arrow 读取；Loader/COPY/目标写和目标管理员凭据留在管线层。依赖保持 `oo-pipeline → oo-connectors → oo-core`；Gateway 查询复制后的 Agent DB，不持源驱动或源凭据。
- 仅复用当前有明确需求的公共数据契约，不提前创建统一工厂、动态插件或二十余源占位框架。原 Plan 的共同接口要求仍待其全部退出，不能由本次模块迁移宣布完成。
- MySQL 5.7 与 8.x 分别实测；本轮真实目标是 5.7.44-log/8.4.10，不推导 MariaDB 或全版本兼容。保持 C1 binlog CDC，批量读取用于基线和对账，不把产品路线改成 C5。

## 本次验证与完成口径

最终固定源码与锁文件必须完成 Linux Rust 1.94 默认 PG / 启用 mysql / 实际消费者构建和相关回归、依赖方向检查；两真实版本验证有界批量读取、类型保真、TLS、只读边界、取消和预算清理。实际主线必须调用新核心，完成源→Arrow→Loader→S0–S10/维护 YAML、Ontology、Pack→发布/复制库→Gateway/CLI/随机 Agent 对话的连贯证据。只搬文件、声明 feature 或组件测试成功均不是完整出口。

已有证据仅可按固定源码、源身份、数据/语义版本及 Pack 的实际适用边界复用；新 binding/generation 不能套用旧问答。运行的中间制品与最终固定 ELF 分开记录。当前仍在开发/整链验证，未新增父项、正式源或整版签收，计数增量 0；总 Goal 和其他源仍暂停。

当前任务：[完整能力卡](../batches/native-luna-full-source-20261015/MYSQL-MAINLINE-BATCH-READER-SOL-MEDIUM-20261016.md)；唯一工程师 `/root/mysql_rust_runtime_sol_medium`，GPT-6.1 Sol medium。[状态更新协议](../UPDATE-PROTOCOL.md) 继续适用。
