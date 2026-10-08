# Self-build S0–S10 收敛与作者退出标准

版本：2026-10-08。依据完整主 Spec/Plan SP07、SP07-W04、原 E56 与 tools/selfbuild-control-api/CONTRACT.md。此文件明确执行边界，不缩小原验收范围。作者唯一 a373；Codex 统筹；独立复审在固定提交后执行。目标是减少非审阅导致的再次续接，不能保证发现率或返工率绝对为零。

## 当前实证缺口

当前原 a373 报告中的成功作业 S0–S8 succeeded、S9 awaiting、S10 succeeded，但作业仍 running 且阶段列 S5；审核通过的字段没有进入 operator.json，field_candidates_applied=false、reseal_count=0、reviewed_unchanged。这不能作为完整 S0–S10 收敛。tests/contract.rs 只做 Clippy 未执行。Identity a6f92 已独立验收，旧报告的“身份未验收”不再是当前依赖；儿童 ledger 的 authenticated=false 不得伪改，必须由实际服务的验证身份/审核审计证明授权。

## 稳定输入与唯一所有权

- a373 唯一修改 tools/selfbuild-control-api/**、自身 evidence/**；其他产品消费者使用固定归档，不读移动工作区。
- 核心 Builder be9acbc23、rules e5427c1a、suggestions d22187ea、ledger 6b3a92c2、identity a6f92 使用完整 SHA 在任务中核对。完整十五条 HTTP 操作不缩水。
- 已装载私有 PG 十万行 fixture 2000/8000/90000 复用，不重载，不连接企业源。不重跑已绿不变基础 campaign。
- Builder LLM grok-4.7 独立配置；密钥仅本地私有文件引用；实际 HTTP、fresh/cache、用量与成本状态保留。Unknown 金额如实记录。
- Pack→对话 policy 标识兼容修复 a399 独占两个 Builder 模块与 ontology-dialogue；不阻原 E56 构建、审核、编译 Pack 的可独立收敛。语义发布属于 a392，不假定编译即发布。

## 实施前输入检查

作者先写 READY-CHECK.json：每个固定 producer 二进制/source SHA、真正可执行版本、私有 PG schema/fixture 行数、身份 issuer/current、表卡片与画像 seal、模型配置、HTTP schema 已发布版本。失败逐项记录。若某项是本卡缺失，作者补齐再开始；若需其他写者，写一个精确接口缺口交协调，不反复启动尚缺同一依赖的全链。该检查为真正私有资源测量，不用人为 ready=true。

## 唯一阶段状态与审核状态

1. S0 catalog、S1画像：真实获准源读取、哈希及来源关联；获准卡片和模型输入成员精确匹配，未知/隐藏字段不泄漏。
2. S2–S4：真实 rules 结果、PII/关系/enum 锚点及 uncertainty 可审核；不自动赋权或认证。
3. S5：真实 Builder LLM 请求、候选验证与持久化，模型输入/candidate hashes/cache/tokens/cost 可追踪。
4. S6–S8：真实建议与来源持久化；generated 未合并、未认证、不自动发布。
5. S9：每一个本次拟进入编译的候选必须有真实权限受控、范围/版本/锚点匹配的显式 approve/reject/defer 决定。approved 描述和列标签必须真正进入 compiler 输入；拒绝和 defer 不得写入 curated。只编译人已审核的范围；不自动为其他列制造批准。
6. S10：业务负责人针对该构建的有效列确认分类；分类与权限分离。S9 本次编译范围未完成时不得提前标全部审核完成。未知/hidden 保持关闭。
7. 只允许真实已执行阶段推进。waiting_review 合法用于人的待办；完整验收的主成功构建必须由真实审核请求收敛为终态，不留下 running/S5 与 S10 succeeded 的矛盾。取消和失败也是独立终态；幂等重放、重启不重新生成已确认副作用。
8. 编译后的完成状态名称依已冻结 schema 的合法状态；必须有可观察 terminal receipt，若原 schema 无表达终态能力，先写最小版本兼容变更和 OpenAPI/hash，再实现，不暗改状态语义。

## 作者必须全部通过的验收项

| ID | 输入与操作 | 可验证结果 |
|---|---|---|
| SB-01 | 正常真实 100k 注册源和画像 | S0/S1 catalog/row count/profile/seal/获准表卡片一致 |
| SB-02 | 实际 grok-4.7 Builder 调用 | provider 响应、严格验证候选、实际请求 hash/用量/成本，缓存首次/命中区分；不是仅 HTTP200 |
| SB-03 | 真实 reviewer 通过一条表描述及一条字段候选，另 reject/defer | 原候选锚点绑定；approve 文字实际见于两角色 Pack；reject/defer 不写入；编译器真正 apply/reseal 证据，不能 field_candidates_applied=false 交付 |
| SB-04 | 真实 owner 完整确认本次可编译分类 | S9/S10 本次审核范围完成；无自动 grant、未知字段隐藏，两个角色 Pack 含被批准内容 |
| SB-05 | compile 和 GET | 作业终态与 stage 统一；固定 Pack hash；published=false；重启 GET hash 相同 |
| SB-06 | 相同输入重复构建/审核/编译 | 输入哈希缓存，实际模型请求计数不增加；同幂等键同 receipt，不同 payload 冲突 |
| SB-07 | 模型不可达/错误响应/校验拒绝 | failed 状态可追踪，无批准审核、确认分类、Pack 编译或发布副作用，已有其他成功构建不受损 |
| SB-08 | 取消正在运行构建 | 终态 canceled；不得取出可用 Pack；真实停止/检查点状态，不伪造后台已停 |
| SB-09 | 身份/项目/角色/CSRF/revision/成员关系负例 | 原十五操作应有必需角色矩阵、无匿名/跨项目写；依赖失败 fail closed；不把 caller JSON 或 ledger child 声明当身份 |
| SB-10 | 契约与生产启动 | Linux Rust1.94 first smoke；contract assertions 真正运行；focused tests/Clippy0；完整服务+binaries/config schema 的重现命令 |
| SB-11 | 完整证据及固定交付 | REPORT/FINDINGS/HANDOFF、各阶段 raw exits、source SHA/evidence SHA、失败尝试保留、owned cleanup 或明确移交资源列表、固定提交 SHA、前端可绑定 fifteen operation handoff |

正例失败不能以缩小审核范围、移除测试、重写 gold、直接写 state、手写 Pack、mock 后端、关闭校验或留待后续交付绕过。

## 作者退出门槛

正常交付退出必须同时满足：SB-01–11 全部 PASS；作者自检 FINAL-AC.json 逐项引用真实文件/命令退出与 SHA；git 固定 owned 提交；完整证据自查零坏/零缺。FINAL-AC 缺项或矛盾即不交付，作者留在同一卡内修复，而非交一份“下一次 builder 正在跑”的报告。最终交付消息只包含固定提交、验收索引、原始测试退出、已移交 API 与必要外部限制。

工具进程可因上下文、服务故障、额度或用户中断被迫结束，不能保证 CLI 永不退出；这类退出标为 interrupted，不算作者交付，也不计非审阅实现返工。恢复优先收现有资源，无重复 builder。

独立复审只针对固定提交核验上述原范围，必要差异边界测试，不反复扩大无关矩阵。发现产品错误计审阅后返工；作者未完成/缺包导致的续接单列，不混入唯一组件审阅返工率。

## 台账与测量

- 首次完整交付率：第一次声明 delivery_ready 的卡中，SB-01–11/证据齐备且可进入固定复审的比例。
- 非审阅返工：因输入/接口/状态遗漏、未跑必需检查、缺证据导致 delivery_ready 后退回；目标接近零，不以改分母达标。
- 中断续接：额度/工具/上下文导致 CLI 退出且未声明 delivery_ready，单独统计；不作为代码完成或返工。
- 独立复审返工：固定提交发现真实缺陷后修复，沿现有唯一组件口径。
- 本卡完成不等于完整 SP07/发布/Agent/发行。那些工作继续原 Plan，不用本文件放宽最终目标。

## 不可遗漏的产品主链：标准 YAML → 审核 overlay → Git 版本 → 发布 → Agent Pack

依据 docs/02-technology-stack.md 的 OOS 真相源决定、docs/06-ontology-layer.md §3.1–3.3、主 Plan SP07-W01/W05/W06、SP02-W02、SP08-W04。每次开工必须读取对应 docs/Plan 当前段落、记录 requirement→接口→真实测试→证据映射，不从已有代码推断完成范围。

| ID | 主链验收 | 必须保留的证据 |
|---|---|---|
| OC-01 | catalog/profile/rules/LLM生成 schema-valid 标准 OOS YAML，ontology.yaml与generated目录真实落盘；JSON/Markdown仅编译产物 | YAML字节/hash/schema版本/来源seal；禁止只有临时JSON/S5响应 |
| OC-02 | 真实权限审核写 reviews/decisions.yaml及curated overlay，编译effective | approve/reject/defer真实人/版本/证据；fieldmerge/列表stableidentity/rejected移除/staleoverride；再次Builder不覆盖curated |
| OC-03 | 审核overlay/config成为实际Git不可变commit | 审核receipt→oldHEAD CAS→commit/tree/blob/配置hash；restart读取同版本，冲突不覆盖；无假commit字符串 |
| OC-04 | 同一Git版本产生immutable release candidate，通过必需G1–G13/角色fence/replay/CAS active | 每G实际结果/生产与privatefixture分离、ACL/角色副本屏障和操作审计；缺安全gate不发布，不把compiled_pack改published |
| OC-05 | Agent实际取得当前active发布版本的对应角色Pack | Pack manifest/sourceGit/release/policy/knowledge/contenthash闭合；缓存tuple/隐藏字段裁剪与撤权；实际Agent工具查询引用此Pack |
| OC-06 | 同一真实十万行旅程贯通，必要失败/重启闭环 | YAML→decisions→curatedGit→candidate→active→AgentPack每边hash对应；模型失败不改审核/Git/发布、CAS冲突不换版本、stale/撤权拒绝 |

S0–S10 E56作者退出仍需SB-01–11，不能将E56 compiled_pack称完整主链。完整主链新增独立集成owner a403，消费固定a373/curated-git-apply/发布API/Agent入口；各原模块owner负责自身真实接口。a403拥有新lifecycle编排模块及证据，不改别人的模块；接口缺口须精确交owner修，不用手写YAML或预置gate等mock通过。OC-01–06没有全部真实证明，主链与完整SP07/08产品能力保持未完成。
