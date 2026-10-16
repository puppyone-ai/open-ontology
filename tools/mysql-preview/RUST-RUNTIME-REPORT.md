# MySQL Rust 产品运行闭环：作者 fixture 交付

2026-10-16。代码 `e3e83b3995c19305de9bc8530c08b0fcc9722b9e`，实现基础 `1afe10bb624cc4d26c53339bfc6e06d9d9606d9b`。两版仍计 MySQL 一个源；正式源增量 0，生产签收 0，Goal/原 Plan 分母不变。root 负责独立验收、默认入口与 registry 切换。

## 交付行为

新增独立 Rust crate `tools/source-onboarding`，入口接受六字段连接文件及已安装的私有 operator 配置。它直接调用原 Rust C1 producer、worker、Loader、认证 Control API、release API、正式 fixture 发布、mapping-reader、只读 Gateway 与 OO CLI；接入、周期调度、语义维护和发布路径不启动 Python 业务脚本。`oo-current` 是 operator fixture 的 Rust CLI 定位器，实际数据调用仍由既有 Rust OO CLI 经 Gateway 完成。

正常输入只含 type/host/port/database/user/secret_ref，不能提交 recipe 或历史收据。原 source/binding/项目、受选表列、SecretRef 解析、ELF pin、合法 S5、审核/角色与 fence 均复用。新客户连接与已安装 fixture 不同即拒绝，不获得 fixture 写授权。

## 实测结果

| 版本 | 当前代 / applied_pos | 行 / 表 / 列 / typed 差异 | 当前 Pack digest | Linux namespace PID | 当前 client token 到期 UTC |
|---|---|---|---|---|---|
| mysql57 | 5 / 94 | 110019 / 5 / 43 / 0 | `b60da0b377b99aa6de922bc93a1748ec5c85b0f5df9a1bc5261c00a7381e5c3f` | 50451 | 2026-10-16T09:34:24.713000+00:00 |
| mysql8 | 8 / 40 | 110019 / 5 / 43 / 0 | `e339adc94e1526c55bf24ef465b769e5abafbec05ba7dec3c0befded584191cf` | 50463 | 2026-10-16T09:34:20.895000+00:00 |

- 两版真实新 I/U/D 各一事务，由 Rust v4 host runner 驱动原 CDC-first → Loader → CAS committed → ACK → 维护 → 正式 test_scope fixture 发布 → 新物理 RO Gateway。原源端 UUID/epoch、逐表 run、原始 transaction frame 与 actual exit 全部保留。
- 最终 v10b 是 Linux aarch64 / Rust 1.94 ELF；两版实际冷进程启动/同 cut 重入、全量独立 typed oracle、画像、同 S5 新维护、发布身份复核、新安全 Gateway 与 Rust OO CLI 预检均 exit 0。原已发布 cut 仅在 request/load/ack/7 文件 pin、目标 published 身份、当前 active/operation 和 Pack 一致时合法复用；没有把旧源或旧 Pack 的问答继承到新身份。
- 旧失联服务留下的两 running task 由新 Rust 入口经正式 `native-step` 接同 task lease generation 2，真实完成 no-change；没有手改 task/epoch/ACK。后续 quiet 保持 g5/94、g8/40，apply=false、ack=false，不增代。原同身份的 IUD/Loader crash/recovery 历史仍保留；未重复一次无必要故障 DML。
- 当前 token 的列拒绝、未选关系拒绝、accepted-query cancel、durable complete WAL 和 rollback 均 exit 0；物理副本实查 `pg_is_in_recovery=true`、`transaction_read_only=on`。
- 四组随机、每组两轮，共八个事实全部 GPT-6.1 Sol / low verified。每版组间 semantic_id 不重叠；银行/selection 未重抽。独立检查逐事实比较 source gold、原 CLI cell、原 QID admit/complete WAL。最终 v10b 与该 QA 的 source、代次、Pack 字节及当前 gold 完全一致，故按授权严格复用，安全修正未新增付费调用。

实际模型用量：51 请求，prompt 117213，completion 2224，total **119437 tokens**。新 S5 0，Grok fallback 0。工程智能体 token 不可独立计量，未宣称达到历史 1/5。

## Secret 边界

最终 Gateway 只有 `/run-current` 与只读 `/fixed` 两挂载，`/run-current` 中密码文件仅 `runtime.pass`（token lookup runtime）与 `data.pass`（RO data）。admin/source Secret、控制挂载和 Docker socket 在 Gateway 中实查不可读。`token_admin_password_file` 只由控制容器 `oo-mysql-connection-whole` 的私有 `/secrets/...` 引用使用，mode 0600；签发到 control-owned 临时文件后只复制 client token。

初版沿用旧 Follow 的 token-admin 文件复制方式；v10b 已修正。v8 两个自有长任务在 binding disable、无 queued/running task 的 ACK 静点按精确 argv SIGTERM，实际 exit 143 已保留。六个本任务创建的 v4/v8 中间 Gateway 已正常 Docker stop，文件/WAL 保留（sleep-infinity 容器的实际 State.ExitCode=137，原已完成查询/WAL不改）；原任务前 legacy Gateway、49 pins、旧 recipe/原报告/证据未改，后续由 root 治理。

## 固定与验证入口

- 源/ELF/构建 provenance：`/tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/source-and-elf-provenance-v10b.json`
- Linux ELF SHA256：`b0808d42b885366d7825aef1ea266d3120a614180dd8e943ba58a57263e84d68`
- 固定 archive SHA256：`5ed90b840d5ec67bac9404c23e60dd5069cd42bd78729d6278105fefc0349921`
- 作者独立重放结果：`/tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/independent-final-replay-v10b/`
- root 可读后独立运行：`python3 /tmp/oo-mysql-rust-runtime-sol-medium-20261016/verify-rust-runtime-v10b.py --out /tmp/oo-mysql-root-independent-NEW`
- verifier SHA256：`317a6ac501266ca4b0ae7e52e662472c3df0a8b03a7921a1c3057daebd0d77a4`；它固定并执行 Secret 隔离探针，输出目录必须新建，拒绝覆盖。
- Linux 1.94 baseline 最小源测试 1/1；新 crate 六项测试（host/Linux）、locked build、Clippy -D warnings 与格式检查均有 raw exit。真实 E2E 证据补足权限、类型、重入和发布行为，未跑全仓库套件或宣称吞吐指标。

原固定 base 6042 + 911392259、archive `92244a6c...` 隔离构建；不读当前 root Cargo/path dependency 的 foreign WIP。未改 root Cargo、旧 Python、原 49 pins、其他源、治理或 registry。辅助 Python gold/caller/独立 Agent 验证器按原授权保留；它们不在新 Rust 产品运行调用链内。

边界：owned synthetic fixture；5.7 官方 amd64 在仿真环境，非性能/原生架构认证。24h 有界服务、1h token、无自动续签、无 boot daemon，审计 collector 未配置，当前证明为本地 durable WAL。任意客户部署仍须其只读权限/ROW-FULL-GTID/保留窗口/身份及 owner、原外部门槛，不由 fixture 结果代签。
