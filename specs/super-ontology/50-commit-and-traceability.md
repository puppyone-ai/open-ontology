# Super Ontology 提交策略与追溯

> 前提：[00 决策记录](00-overview-and-decisions.md) D36、D39；[30 闭环与 Sprint](30-closed-loops-and-sprints.md)。
> 本文在两个仓库现有约定的基础上做加法，不推翻现有约定：
> - OO：`docs/engineering/collaboration.md`（只用 owner 身份、trailer 只写 `Plan:`、全绿才普通 merge、不 force push）；
> - Puppyone：`AGENTS.md`（`main` 发版、`qubits` 集成、`codex/<task>` 任务分支、Conventional Commits、远端操作需授权）。

目标只有一个：**任何一行代码，都能在一分钟内回答“它属于哪张任务卡、哪个闭环、依据哪条决策、在哪里验收过”。** 反过来，任何一张任务卡，都能列出它的全部提交、PR 和证据。

## 1. 标识体系

所有追溯都靠下面这组 ID 串起来。ID 一经分配不复用、不改名。

| ID | 格式 | 例子 | 定义在 |
|---|---|---|---|
| 决策 | `D<n>` | `D26` | 00 §4 |
| 闭环 | `L<n>` | `L4` | 30 §2 |
| Sprint | `S<n>` | `S6` | 30 §2 |
| 后端任务卡 | `SO-B-S<n>-<nn>` | `SO-B-S6-03` | 10 分册（文中写作 `B-S6-03`，trailer 中加 `SO-` 前缀，避免和 v0.2 的 `S6-B-01` 之类混淆） |
| 前端任务卡 | `SO-F-S<n>-<nn>` | `SO-F-S6-02` | 20 分册 |
| 形式化验证任务卡 | `SO-V-S<n>-<nn>` | `SO-V-S3-02` | 80 分册 |
| 并行板块 | `WS-<名>` | `WS-BE3` | 70 分册（不进 trailer，只用于排期） |
| spec 改动 | `SO-SPEC` | — | 本目录 |
| 闭环签收 | `SO-SIGN-L<n>` | `SO-SIGN-L4` | 30 §3 |
| 契约版本 | `docs-api/<major>.<minor>` | `docs-api/1.3` | 10 分册的控制 API 契约 |

## 2. 分支模型

### 2.1 OO（`puppyone-ai/open-ontology`）

```
master ────────────────●────────────────────────●──────────▶   （每个闭环签收后合一次）
                       ▲ merge (SO-SIGN-L2)     ▲ merge (SO-SIGN-L3)
feat/super-ontology ──●──●──●──●──●──●──●──●──●──●──▶          （本轮的集成分支，长期）
                        ▲     ▲        ▲
so/B-S3-01-docling-sidecar   so/B-S3-04-state-machine   …      （任务分支，一卡一枝，短期）
```

- **集成分支** `feat/super-ontology`：从 `origin/master` 拉出，worktree 为 `oo-v02/super-ontology`。只接受任务分支的合并和主线程的共享改动（见 §4）。
- **任务分支** `so/<卡号>-<短描述>`，例如 `so/B-S3-01-docling-sidecar`：从集成分支拉出，一张卡一个分支、一个 worktree（`oo-v02/so-<卡号>`），合并后立即删除分支和 worktree。
- **同步方向**：master 有新提交时，用普通 merge 把 master 合进集成分支，不 rebase 已推送的历史。任务分支在推送前可以 rebase 到集成分支的最新提交；推送后不再改写。
- **进 master 的时机**：只在闭环签收时。PR 标题写 `Super Ontology L<n>: <闭环名>`，PR 必须全绿（含 `aggregate`、`oo-test-fast-l1-l2` 以及 30 §4 的横切门禁）。文档流水线在 Cargo feature `docs` 后面，默认关，保证 master 上的现有用户不受影响。

### 2.2 Puppyone（`puppyone-ai/puppyone`）：开发期间零影响，最后一次性提 PR

owner 要求：**本轮工作不能对 Puppyone 的 `main` 造成任何影响**；先在自己的分支上做完、测透，效果完全 OK 之后，再集中提一个 PR 交给上级审核。所以 Puppyone 侧的规则与 OO 不同：

```
origin/main ──────────────────────────────────────────────▶   （只读：不推送、不合并、不打标签）
     │ 本地拉出（只读取）          ▲ 定期 merge 进来（本地）
     ▼                              │
codex/super-drive ─●──●──●──●──●──●──●──●──●──────▶  ……  ──▶ 一个 PR（时机由 owner 决定）
                    ▲     ▲
codex/super-drive-F-S3-01-upload-queue  …                    （任务分支，只在本地）
```

- **集成分支** `codex/super-drive` 从本地的 `main`（0.3.27，含 ShareWizard）拉出。拉分支只读取 `main`，不改它。**不同步、不推送 `qubits`**（O4 原建议撤回）。
- **任务分支** `codex/super-drive-<卡号>-<短描述>`，从集成分支拉出，合回集成分支，全部在本地完成。
- **跟进上游**：`origin/main` 有新提交时，用普通 merge 合进 `codex/super-drive`（本地操作，不影响 `main`）。不 rebase 已推送的历史。
- **开发期间禁止**：向 `main` 或 `qubits` 推送、合并、打标签、开 PR；修改仓库设置或 CI 配置使其作用于 `main`。
- **备份**：`codex/super-drive` 可以经 owner 授权推送到远端作为备份（推一个功能分支不影响 `main`），但在 owner 宣布“效果完全 OK”之前不开 PR、不请求审核。
- **闭环签收**：Puppyone 侧不合并到任何共享分支。签收只在 `codex/super-drive` 上打本地附注标签 `super-drive/L<n>`，并在 OO 的证据目录记录 sha（§7）。
- **最终 PR**：
  - 时机：owner 在某次 owner 验收包（60 分册 §3.4）上确认“效果完全 OK”之后，最早是 L4；如果 owner 希望把 M2、M3 也一起交，就继续在分支上做，到时再提。
  - 内容：`codex/super-drive` 上的全部提交，保留逐卡提交与 trailer（不 squash），便于上级按卡审核。
  - 目标分支：按 Puppyone 维护者当时的要求；`AGENTS.md` 默认是 `qubits`。`qubits` 目前落后 `main`，如果届时还没同步，PR 描述里写明并请维护者决定先同步还是改目标分支。
  - PR 描述附：spec 链接、每个闭环的签收证据、最后一次 owner 验收包的 `BUILD.txt`、T1/T2/T3 结果、形式化验证报告（80 分册）、已知问题。开 PR 本身是远端操作，需要 owner 授权。
  - `superDrive` flag 默认关，合入后对现有用户无影响。

### 2.3 合并方式

两个仓库都用**普通 merge（`--no-ff`）**，不用 squash，也不用 rebase merge。原因是 squash 会把一张卡的多个提交压成一个，`Plan:` trailer 和中间提交里的设计理由就丢了。禁止 force push。

## 3. 提交粒度

1. **一个提交只做一件事。** 一张卡通常拆成 2–6 个提交，例如：先加类型与接口 → 再加实现 → 再加测试 → 最后接到 CLI 或界面。
2. **每个提交都能编译、能通过它所触及模块的测试。** 这样 `git bisect` 才有用。实在做不到（例如跨模块的大重命名），在提交正文里写 `Bisect: skip` 并说明原因。
3. **不混合：** 功能与格式化、功能与依赖升级、功能与重命名，都分开提交。
4. **生成文件跟着源头走：** 改了 CLI 或错误码，要在同一个提交里重新生成 `gen_cli_reference.py`、`gen_error_reference.py` 的产物（OO 现有约定）。
5. **迁移单独提交：** 每个 `_oo_docs` 迁移文件（从 0137 起编号）独占一个提交，由主线程提交（§4）。
6. **测试数据单独提交：** 评测集、红队文件、golden 快照的新增或修改，单独提交，并在正文里写明标注是谁改的、为什么改。golden 快照的变化必须能解释，不能为了让测试通过直接重录。

## 4. 共享文件的所有权

下面这些文件只能由主线程提交，执行者需要改的话，在报告里写明需求，由主线程统一改：

| 仓库 | 文件或目录 |
|---|---|
| OO | 根 `Cargo.toml`、`Cargo.lock`；`crates/oo-pipeline/migrations/`（以及新增的 `_oo_docs` 迁移目录）；共享 trait 与类型（`DocumentSource`、`oo.doc-ir/v1` 的 Rust 类型、控制 API 的 schema 文件）；`supply-chain/`；`execution/`；`specs/super-ontology/00-*.md` |
| Puppyone | `package.json`、`package-lock.json`；`electron/main.mjs` 中的服务组装；`trusted-ipc.mjs` 注册表；`workspaceSurfaceRegistry.ts`；`flags.json`；`locales/manifest.json`；`tests/config/discovery.mjs`、`tests/support/runners/run-desktop-e2e.mjs`、`scripts/release-checks/checks.json`；`shared/desktop/application-identity.mjs`、`release-identity.mjs`、`electron/main/build-info-service.mjs`（验收渠道） |

## 5. 提交信息格式

### 5.1 OO

沿用仓库现有风格：英文祈使句标题，不带 Conventional 前缀；正文写“为什么”；末尾 trailer。

```
Parse XLSX sheets into doc-ir blocks with cell-range anchors

calamine only stores a merged region's value in its top-left cell, so
the reader expands merged regions before header detection; otherwise the
header row looks sparse and the table detector splits one table into two.
Anchors use Sheet!A1:C9 so the BRIEF can link to the exact range.

Plan: SO-B-S3-02
Decision: D14
```

### 5.2 Puppyone

沿用 `AGENTS.md` 的 Conventional Commits。scope 固定为下面几个之一：`super-drive`（渲染层）、`super-drive-main`（主进程服务与 IPC）、`super-drive-i18n`、`super-drive-test`。

```
feat(super-drive): show per-file processing state in the drive list

Plan: SO-F-S3-02
Requires: open-ontology@3f2a9c1
Contract-Version: docs-api/1.1
```

### 5.3 trailer 全表

| trailer | 何时必填 | 含义 |
|---|---|---|
| `Plan:` | **每个提交都必填** | 任务卡号，或 `SO-SPEC`、`SO-SIGN-L<n>`。一个提交只写一个 `Plan:`；确实服务两张卡时，拆成两个提交 |
| `Decision:` | 实现、改动或偏离某条决策时 | 例如 `D26`；偏离决策必须先改 00，再在实现提交里引用 |
| `Contract-Version:` | 改动或首次使用控制 API 契约时 | OO 侧在改契约的提交里写新版本号；Puppyone 侧写它依赖的版本号 |
| `Requires:` | 跨仓依赖时 | `open-ontology@<短 sha>` 或 `puppyone@<短 sha>`，指向对方仓库已推送的提交 |
| `Evidence:` | 签收提交与修复已签收问题时 | 证据目录，例如 `testing/super-ontology/evidence/L4/` |
| `Migration:` | 新增迁移时 | 迁移编号，例如 `0137` |
| `Bisect:` | 提交无法单独通过测试时 | `skip` 加原因 |
| `Fixes:` | 修复某个已合入的提交时 | 被修复提交的短 sha |
| `Formal:` | 改动了某个形式化模型覆盖的协议或函数时 | 模型或证明的编号，例如 `FM3`、`KP2`（80 分册）；模型必须在同一个 PR 里同步更新 |

**身份规则**：提交人和作者都只能是 owner（`Redux0223`）。**不加** `Co-Authored-By`，也不加任何 agent 署名。这条优先于任何工具默认的署名提示。

## 6. 跨仓契约变更的顺序

控制 API 是两个仓库唯一的耦合点，按下面的顺序改，保证任意时刻两边都能各自通过测试：

1. OO 提交契约变更（schema 文件 + 实现 + 契约测试），版本号按规则升级：只加可选字段或新端点升 minor；删除或改语义升 major（M3 之前不允许 major 变更）。trailer 写 `Contract-Version: docs-api/1.x`。
2. OO 侧同时保留旧行为，直到 Puppyone 侧跟上。
3. Puppyone 提交对新契约的使用，trailer 写 `Requires: open-ontology@<sha>` 和 `Contract-Version: docs-api/1.x`。
4. Puppyone 的契约 mock 由 OO 的 schema 文件生成，不手写。版本对不上，契约测试直接失败。

## 7. 闭环签收的提交与标签

每个闭环签收时，按顺序做三件事：

1. **证据提交**：在 OO 集成分支上提交 `testing/super-ontology/evidence/L<n>/`（证据清单、命令与输出、评测报告、截图或录屏的链接、已知问题），标题写 `Sign off Super Ontology L<n>: <闭环名>`，trailer 为 `Plan: SO-SIGN-L<n>` 和 `Evidence: testing/super-ontology/evidence/L<n>/`。证据里记下两个仓库被签收的 commit sha。
2. **合并**：OO 集成分支 → `master`（视情况，全绿才合）；Puppyone **不合并**，只记录 `codex/super-drive` 的签收 sha（§2.2）。
3. **打标签**：OO 打附注标签 `super-ontology/L<n>`，指向合并提交；Puppyone 在 `codex/super-drive` 上打**本地**附注标签 `super-drive/L<n>`。两个标签的说明里互相写对方仓库的 sha 和证据路径。推送 OO 标签需要 owner 授权；Puppyone 标签开发期间不推送。

这样，任何一个闭环都能用标签一步回到“两边当时签收的确切版本”。

> D80：阶段一的签收只涉及 OO（`super-ontology/L<n>`）；阶段二 Desktop 会合时在 Puppyone 打本地 `super-drive/L<n>`，证据追加到 `evidence/L<n>/desktop/`（30 §7）。

## 8. 回滚与修复

- 已推送的提交只用 `git revert` 撤回，不改写历史。revert 提交保留原提交的 `Plan:`，并写清原因。
- 已签收闭环里发现的问题：开一张修复卡（编号接在所属 Sprint 后面，例如 `SO-B-S6-09`），提交带 `Fixes: <sha>`；如果影响验收项，下一次签收的证据里写明重新验证的结果。
- 不能为了绕过门禁而 revert 测试或放宽阈值。

## 9. 台账

`execution/super-ontology/ledger.md` 由主线程维护，每张卡一行，每个闭环签收追加一行：

| 卡号 | 闭环 | 状态 | 分支 | 提交（首 → 末） | PR | 证据 | 备注 |
|---|---|---|---|---|---|---|---|
| SO-B-S3-02 | L3 | merged | so/B-S3-02-xlsx | a1b2c3d → e4f5a6b | #21 | evidence/L3/xlsx.md | — |

状态只允许：`todo`、`in_progress`、`review`、`merged`、`signed`（所属闭环已签收）、`dropped`（写原因）。

## 10. 自动检查（L1 引入，作为横切门禁）

在 L1 的 S1 里交付两个检查脚本，挂进两个仓库的 CI：

1. **trailer 检查**：对 `<base>..HEAD` 的每个非 merge 提交，要求：
   - 恰好一个 `Plan:`，值匹配 `^SO-(B|F|V)-S\d+-\d{2}$|^SO-SPEC$|^SO-SIGN-L\d$`（本轮之外的提交按仓库原规则检查，不受影响）；
   - 不出现 `Co-Authored-By` 或其他 agent 署名；
   - 作者与提交人是 owner 的身份；
   - 新增迁移文件的提交带 `Migration:`；改了控制 API schema 文件的提交带 `Contract-Version:`。
2. **台账一致性检查**：台账里标为 `merged` 或 `signed` 的卡，必须在集成分支上找到至少一个对应 `Plan:` 的提交；集成分支上出现的每个 `Plan:`，台账里都要有这张卡。

## 11. 常用追溯命令

```bash
# 某张卡的全部提交
git log --all --grep '^Plan: SO-B-S3-02$'

# 某个闭环（S3、S4）的全部后端提交
git log feat/super-ontology --grep '^Plan: SO-B-S[34]-'

# 一行代码属于哪张卡
git log -L 120,140:crates/oo-docs/src/compile/brief.rs --format='%h %s%n%(trailers:key=Plan,valueonly)'

# 某条决策在哪些提交里落地
git log --all --grep '^Decision: D26$'

# 两个仓库在 L4 签收时的版本
git show super-ontology/L4
```
