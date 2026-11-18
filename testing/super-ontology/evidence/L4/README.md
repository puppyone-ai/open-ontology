# L4 本体与共享 · OO 侧证据（D80 阶段一，**未签收**）

状态：**未签收**。原因：`conformance_share`（FM6）在全量 live 套件的负载下于 4cee27056 失败过一次，单独运行通过；根因修复正在进行，修复并复验之前不签收。OO `feat/super-ontology` @ 4390b2cb4。独立复核运行于 `5f9061df6`（分支 `so/V46-signoff`，2026-11-18，验证员与执行者分离），结果 `result-verify-signoff.json`：状态“通过，其中一次失败已在 e877ac64d 修复并复验”。L4 CLI 验收包（30 §7.2 的 owner 验收包）见 `ACCEPTANCE-CLI.md`，在 `b2386f379` 上重跑 58/58。Desktop 部分（Desktop 内点开出处、分享面板、待确认队列页、T2 P21）按 30 §7 在阶段二补齐。

## 独立复核里的失败、更正与复跑（如实记录）

- `conformance_share`（FM6）：4cee27056 上全量 live 套件运行时失败 1 次，单独运行通过（记录见 `../L8/result-verify-signoff.json` 的 `vectors_on_runs`，该次运行带向量；根因未查明，修复进行中）。

- `extract_llm_stage_live` 在 `5f9061df6` 失败（等待 `published` 超时，复跑同样失败）；原因是测试没有适配 LLM 的重试节奏（5 s × 3），在 `e877ac64d` 把测试改为等待约 65 s 的重试计划，随后在新建的独立 worktree 上 116 s 通过。这是**测试侧的修改**，产品代码没有变。同一提交还让 D63 检查允许块文本向量表（0144 的 `block_vector.embedding`）。
- L4 CLI 验收在 `5f9061df6` 记录为 57/58：第 9 项“重启后文件仍为 published”拿重启后的数量与固定的 24 比较，而重启前实际有 25 个已发布；此前记录的“10/10”是错的，已更正。`516b239c8` 改为与重启前的数量比较，在 `b2386f379` 重跑 58/58。
- 53 个 live 目标：51 通过、1 个跳过（`download` 需要 `--with-s3` 的 RustFS/MinIO，未运行）、1 个失败即上面的 `extract_llm_stage_live`（修复后通过）。
- 回归门禁（核心类型 F1 ≥ 上次签收 − 0.02）：**没有机器可读的上次签收基线**，只能与 `eval/README.md` 的表格逐项相比（在四舍五入内一致）。
- 评分集独立性：V2 审计发现给 golden 加部门组织后 holdout 与 golden 共享了 7 个部门词（`test_gen.py` 失败）。holdout 已用自己的部门词重生成（种子 20270301，`--check` 0 差异），holdout 数字在重生成后的集合上重跑；旧结果（事实 P/R 0.896/0.801）已被取代，原因与差异见 `eval/README.md`。

## 30 §L4 验收（OO 侧口径，30 §7.2）

| # | 验收项 | 结果 | 证据 |
|---|---|---|---|
| 1 | 核心类型精确率 ≥ 0.85、召回率 ≥ 0.75（配模型）；不配模型时单独列出且不报错 | 通过（已发布视图，模型 `gpt-6.1-sol`）：golden 实体 P 0.9892 / R 0.92，事实 P 0.8995 / R 0.8228；holdout 实体 0.9884 / 0.8947，事实 0.904 / 0.811。不配模型：golden 实体 0.9885 / 0.86，事实 0.9797 / 0.6346；holdout 实体 0.9878 / 0.8526，事实 0.9781 / 0.6411，无报错、全部文件 `published`。**不配模型的事实召回 0.63–0.64 低于 0.75，但规格只要求配模型达标，这里单列。** 候选视图（含等待人工的模型提议）实体精确率 0.744（golden）/ 0.726（holdout）不达 0.85，已发布视图才是 Pack 与实体列表所见，候选只是待确认队列的输入。事实召回随模型输出波动（模型 temperature 0 下不同次运行有出入） | `result-verify-signoff.json`、`eval/README.md`、`eval/` |
| 2 | 红队 0 命中（CI 门禁） | 通过：独立复核 `super_ontology_redteam.py` 10/10；执行者门禁运行扫描 5 个范围 × 3 个任务共 466 个文件 0 命中，对照组（含红队文件的包）出现 4 次，红队 docx 也被完整解析；5 个对抗范围被泄漏守卫拒绝 | `result-verify-signoff.json`、`redteam-gate.json`、`redteam-gate.txt` |
| 3 | BRIEF 出处覆盖率 100%；每个 `[src:…]` 能在 Desktop 里点开定位原文 | OO 侧通过：每个 `[src:]` 都命名范围内文件且能解析到块锚点（93 个标签，0 个未解析，0 个范围外）；Desktop 内点开定位为**阶段二** | `result.json`（执行者）、独立 `super_ontology_l4.py` 70/70 |
| 4 | 确定性：同一本体版本、同一参数，Pack 逐字节相同 | 通过：两次 mount 57 个文件除 `SHARE.json` 外逐字节相同，`SHARE.json` 去掉 share id、时间与 refs 后相等，`pack_digest` 相等 | `result.json`、独立 `super_ontology_l4.py` |
| 5 | 编译单个 Pack p95 < 2 秒（1000 文件 Drive，预算 800） | 通过：release 构建 p95 0.539 s、p50 0.419 s、最大 0.636 s（60 次）；索引 1000 文件 18.6 s。基准文件由执行者产出，独立复核的 `super_ontology_l4.py` 读取并断言它 | `bench.json` |
| 6 | 待确认队列：按来源整体接受 100 条 ≤ 1 次操作；接受后 5 秒内进新本体版本 | 通过（API 层）：按来源接受 100 条是一次 `review:decide`、一个事务、一条 `accept_source` overlay（`review_live::thresholds_queue_and_accept_by_source_in_one_operation`，live 目标中通过）；接受到新版本 1.73 s（API 加 worker）。CLI 示例资料里只有 1 条冲突，`accept-source` 返回 `accepted 0`，即 CLI 上没有 100 条的演示；Desktop 待确认队列页为阶段二 | `laneC-L4-S5` 报告、`ACCEPTANCE-CLI.md` 第 6 步 |
| 7 | Share：过期后 `refs/` 失效；撤销后本地挂载目录被删；列表显示正确 | 通过：过期共享的挂载由守护进程在 2.1 s 内删除，两种凭证都 410，撤销返回 `local_removed` 与“已读内容无法收回”提示，列表正确显示 `expired`/`revoked`，过期共享 `check` 退出 30 | `result.json`、独立 `super_ontology_l4.py`、`ACCEPTANCE-CLI.md` 第 12 步 |
| 8 | 端到端演示：理念文档第 4 节第 1–3 步与第 6 步（不含 Suggest） | 通过（CLI 形态）：`super_ontology_cli_acceptance.py` 在 `b2386f379` 58/58，覆盖启动、26 个文件 64 s 内落定（24 `published`、1 `encrypted`、1 `unsupported`）、实体与纠错、待确认冲突、同步、mount、BRIEF、P21、撤销、重启、删除 | `cli-acceptance-run.json`、`cli-acceptance-run.txt`、`ACCEPTANCE-CLI.md` |
| 9 | 轻量漂移（D50）：发布新版本后 5 秒内引用旧决定的 Share 标为 `drifted`；刷新后旧的转 `refreshed` | 通过：`drifted_at − published_at` 0.023 s；从放入文件到 `drifted` 6.3 s（含解析流水线）；`oo context status` 显示 `drifted`，守护进程追加 `UPDATES.md` 的 warn 条目；刷新得到新 Share 并挂载，`check` 回到 0 | `result.json`、独立 `super_ontology_l4.py` |
| 10 | 合并与取代规则（D51） | 通过：规则确定的取代自动执行（1 条事实被结束，没有人工介入）并进当日摘要；被活跃 Share 使用的旧事实被取代时队列有 `supersede_in_use` 提示。LLM 候选只进待确认（阈值逻辑由 `review_live` 与 FM5 的 D51 一致性覆盖）。V2 审计另提示：`temporal_live.rs` 的断言由“不切割被 Share 使用的旧事实”翻转为“切割照常、只留提示”，依据 D50/D51 文字，**需要 owner 确认 D51 是“提示但不阻断”** | `result.json`、V2 审计 |
| 11 | 持续一致性（00 §5.6） | 通过：新鲜度令牌——`--require-complete` 对未处理完的范围退出 7 且不挂载，处理完后成功，BRIEF 头部写明覆盖；包内一致——除 `UPDATES.md` 外字节不变（changed: []）；变更必达——纠错后 1.0 s 内 `UPDATES.md` 追加 `block`，`check` 退出 20，引擎不可达退出 40（fail closed），warn 退出 10；任务组——同组成员单独刷新退出 8，整组刷新两成员到同一新版本；完整性约束——同一项目两位负责人进 `constraint_conflict`，不带 `--winner` 接受被拒（`DOCS_REVIEW_CONSTRAINT_CONFLICT`）；用户纠错——标记错误后新版本发布、受影响 Share 收到更新，且新包不再含错误事实。任务内带 Share 凭证的查询读 Share 版本（D61）由 live 目标覆盖，未单独写入脚本 | `result.json`、`cli-acceptance-run.json`、独立 `super_ontology_l4.py` |
| 12 | 演示 T2 场景 P21（两个并行 Agent，用到的决定被推翻后提交被拦住） | CLI 形态通过：同组两个 Agent，决定被标错后 2.5 s 内 `UPDATES.md` 出现 `block`、`check` 退出 20，组内刷新后两边退出 0，Drive 文件未被改动。Desktop T2 打包黑盒为**阶段二** | `ACCEPTANCE-CLI.md` 第 11 步 |

## 横切门禁（L4，OO 侧）

- 形式化（80 §8 L4，D81 修订）：FM2、FM3、FM4、FM5、FM6、FM8、FM10、FM11 的 FAST 验证无反例，live 一致性测试全部通过，kani-fast `oo-docs` 20/20、`oo-core` 4/4（KP1、KP2、KP5–KP8、KP10）；deep 只作参考。独立复核确认 `formal.md` 与 D81 修订后的门禁一致。**未做：KP3、KP4（没有可证的代码）**；FM8 的三个不变量超出 FAST 预算，用分解、模拟和回放覆盖而非 Apalache；FM6 回放只覆盖纯新增发布，FM5 合并与 FM11 身份约束没有回放。详见 `formal.md`。
- 形式化过程中发现并已修复 4 类缺陷（FM5 待确认队列、FM11 同次运行矛盾、FM10 纠错未计入覆盖、FM6 迟到成员回填）；报告另记录未修的发现：实时流不会为已连接客户端跳过被清除的事件（尚无清除作业）、`state::transition` 没有版本栅栏、`PATCH /shares` 返回 405、D58 留存作业缺失——这些以 `laneA-L4-formal` 报告为准，签收时未逐项重验。
- 红队、确定性、出处覆盖：见验收 2–4。

## 已知缺口与 owner 事项

- V2 审计（`acf198b17`）：`super_ontology_l4.py` 的 D66 排除过宽（排除所有目录名为 suggest 的 md，应收窄为精确路径），属于需要收窄而非放宽；`super_ontology_bench.py` 的 scale 路径不断言 PG ≤ 1 GiB（L8）；约 14 个 `#[ignore]` live 目标没有持续门禁入口。
- 26 条 `secret-review.json` 新增条目均为代码作者自审（V2 审计），4 条原独立审阅被覆盖，需要 owner 安排非作者复核或在签收里注明“自审、待独立复核”。
- 模型配置（验收 1 的“配模型”）使用 owner 的端点与密钥文件，只以路径引用；ACCEPTANCE-CLI 第 15 步（配置模型）没有脚本化也没有重跑。
- 镜像锁：本机本地镜像被后续构建覆盖，CLI 验收用只改了镜像摘要的 `deploy/docs` 副本运行，仓库里的锁文件未改动，需集成线程在重建镜像后重新锁定。
- L4 签收后发布构建打开 `docs` feature 并标为实验功能（D41），npm 发布由 owner 操作；打包 App 的 owner 验收包为阶段二。
