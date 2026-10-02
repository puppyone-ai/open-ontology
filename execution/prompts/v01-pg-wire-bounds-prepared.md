# R2-W prepared：SQLx 接收分配上限

状态：准备卡，尚未派发。派发时必须另写 UUID、固定输入 SHA/tree/parent、实际句柄、槽位和精确写路径。仅 Grok 4.7 xhigh 执行，无子 agent/worktree。不得凭此卡启动 builder。

输入：a118-stream-analysis-receipt.md 及 a118 三报告；沿用 SQLx 0.9.0 patched trees、bytes 1.12.1。目标为接收上限机制及测量，非 portal、connector 流或完整 reader。

## 接口与写权

候选路径仅 vendor/sqlx-postgres/src/connection/stream.rs、connection/mod.rs、vendor/sqlx-core/src/net/socket/buffered.rs、vendor/PATCH.md，以及相应同文件 Rust 单元测试。公开导出或 typed error 需要其他路径时先提交明确最小路径方案，由协调者冻结后才能改；不允许静默扩写。不得改 manifest/lock、配置、connector 业务源码、类型 canonical、其他 evidence。

默认关闭保持兼容；显式 bounds 在消息长度导致 reserve 前拒绝。覆盖 DataRow 字段/行/列数与所有控制消息；拒绝携带静态 typed reason，无服务器文本或值。失同步后不得自动 drain。高水位从实际 reserve/capacity 读取，而非复述配置参数。精确定义总消息长度、payload、前缀和累计计量；每个加法须检查溢出。

协调者核对现有源码后的额外约束：recv_unchecked 明确要求完整消息到达前不改 buf，以保持 cancel-safe。分段读 DataRow 不得把已消费前缀和累加状态只保存在可被丢弃的 future 栈上；须保留可恢复解析状态，或者取消后可靠标记失同步并禁止连接复用。以在每个 await 边界丢弃 future 的测试证明，不得仅用正常完整帧测试代替。connection 模块和 stream 均不是公开模块，若新增公开类型必须通过 sqlx-postgres/src/lib.rs 的最小重导出接入；typed error 不能靠 Protocol 字符串前缀识别。公开导出/error 精确路径在激活前冻结，此 prepared 卡不授予新增路径写权。

## 独立测试矩阵

- 分片头/分片字段前缀、NULL、零长度、负非法长度、截断、长度溢出和行剩余长度不一致。
- 上限恰好通过、加一拒绝：消息/字段/累计行/列数；1600/1601 列。
- 超长 Notice/Error/ParameterStatus 等非 DataRow，在 body reserve 前拒绝且不泄露文本。
- 同一 socket 多帧/overread：接受帧后紧跟拒绝帧，记录实际 capacity 与 reserve 轨迹，证明未为宣称超长 body 分配。
- 分段接收各 await 边界取消后再读：恢复时完整字段无遗漏/重复，或明确失同步且拒绝复用；禁止成功返回错位消息。
- 默认关闭兼容回归；启用上限拒绝后不能 wait_until_ready 排空或误成功。
- 用脚本化 Rust socket/protocol peer 发送长度前缀和有限字节，不分配真实巨大 payload；断言观测到的底层分配轨迹和读量，不能只镜像 if 分支。

第一步遵循 linux-msrv-focused-checks.md：Linux arm64 Rust 1.94、固定 image、locked fetch、离线局部测试/Clippy，至多一个任务 builder；本任务无需 PostgreSQL。若 vendor workspace 不能直接执行 tests，明确实现可运行 Rust 测试入口及精确授权路径，禁止留下未执行单元测试当作通过。无 C/Python 测试 harness。

输出：精确 owned-path 本地实现提交，REPORT/FINDINGS/HANDOFF、原始命令/退出码、失败保留、allocation 观测和 hashes。cleanup 仅 UUID 所有资源。禁止 push/release、许可豁免、foreign cleanup。交付后立即固定 SHA 独立复审；仅通过 R2-W 子项，不勾完整 reader。
