---
name: codex-grok
description: Use when a Codex coordinator delegates nontrivial repository work to Grok CLI, manages parallel executors, or resumes an evidence-driven implementation plan. Also use for fixed-commit review and executor handoff; do not use it to authorize Grok when the user has paused new dispatch.
---

# Codex–Grok coordination

Adapted from the user's original codex-grok skill. Codex owns scope, contracts, task boundaries, scheduling and final acceptance. Grok normally executes frozen tasks; the latest human-selected executor policy takes precedence. Never start a nested Codex CLI or an executor's own subagents.

## Start with current facts

Read the repository handoff, goal and current control snapshot before historical logs. In this repository, start at [engineering index](../../../execution/INDEX.md); after every delivery/review/integration transition follow [cross-file update protocol](../../../execution/UPDATE-PROTOCOL.md). In Open Ontology use `execution/HANDOFF.md`, `execution/GOAL.md`, `execution/control-state.json`, then relevant Spec/Plan sections. Historical tables and process counts are not current task ownership. Verify local CLI flags with `grok --help` if the recipe or CLI changes.

## Freeze a complete capability

Each task identifies its Plan/REQ/AT, actual input, frozen dependency SHA, exact output/API, exclusive write paths, resource allowance, success and necessary failure cases, fixed-commit/evidence deliverables and cleanup ownership. Use [task template](references/task-template.md). A worker must implement and demonstrate the assigned end-to-end capability; starting a builder, emitting a schema, or leaving a fixture is not delivery.

Shared interfaces/root Cargo/lock/migrations have one writer. Consumers build fixed archives; never compile another worker's half-edited workspace or restore foreign changes. Transfer ownership explicitly only after the former writer and any background mutator stop. Background computation may transfer with a named resource receipt; the successor collects it before launching anything new.

## Dispatch and wait

Use [CLI and lifecycle](references/cli-and-lifecycle.md) for new/resumed sessions. Separate UUID, prompt, logs, evidence, cache and resource names per task. Prefer a fresh session with compact current inputs when repeated compaction or stale history outweighs resume continuity. Frontend/session-specific user constraints remain exceptions.

Read current user limits; do not hard-code project concurrency into other repositories. For this project, ordinary observation is one 15-minute wait followed by one batch over known live handles. Notifications or new user instructions may end the wait earlier. No repeated short polls or live-log tailing. CLI exit and background completion are separate states. Quietness and elapsed time alone do not justify killing or restarting a worker.

## Accept fixed evidence

Use [evidence and review](references/evidence-and-review.md). Read REPORT/FINDINGS/HANDOFF, raw exits, exact source/binary/dependency identities and cleanup receipt; independently verify hashes and decisive behavior. Author tests, fixed independent review, product integration and release readiness are different levels. Preserve failed attempts. Do not change gold, hide unsupported cases, reuse another run's results, or count HTTP200/CLI0/zero-test suites as a closed product capability.

Review the declared boundary. Repeat only changed behavior, a new failure or a demonstrated gap; do not expand into unrelated security matrices. Existing source-readonly, credential and publication boundaries remain mandatory. Source/role/model/Pack/import identities must describe the actual run, not a plausible configuration string.

## Continue without falsifying completion

After delivery, commit exact owned paths, dispatch fixed review, update ledger/Plan/TODO/changelog/control snapshot, release verified reservations and schedule newly unblocked work. Prioritize critical-path product capabilities while other independent Plan work continues. Count delivered functions, accepted work and rework separately from code volume.

Never mark the whole goal complete from a local milestone, deadline or low budget. Preserve human/customer/CI/runsc/release prerequisites separately. Respect an explicit pause. If executor switching is authorized, stop only the identified managed writer, preserve code/resources, confirm its exit and transfer exact ownership. The original skill's two-failure automatic takeover and 30-minute quiet-run rule are superseded by human policy and the lifecycle above, not universal triggers.

## Current human review timing (2026-10-10)

Routine function development does not require a separate review after every delivery. Complete frozen task exits, meaningful actual tests, fixed implementation/evidence and resource receipts remain mandatory. Coordinator may allow connected development using such author-complete immutable inputs, explicitly recording deferred independent review. Review critical milestones and complete/connected product loops independently at their fixed SHA. This direct human policy overrides earlier per-function review scheduling; it does not turn author self-tests into independent approval or source/parent/release acceptance. See execution/review-policy.json.

Human scheduling update2026-10-10: reliable executor target15, ordinary event wait900 seconds, exits immediate. Rust actual global10/task1 and Docker16CPU remain; do not equate15 CLI sessions to15 compilers or bypass ownership/dependencies.

### 2026-10-10 最新用户闭环交付与等待要求

每数据源唯一自主工程师负责完整产品链；重复driver/consumer工程师合并，代码/失败/资源归属保留。每次工作收集真实完整交付，不用小功能状态和CLI数量替代闭环。固定等待调用clock.sleep900秒（可被新输入中断），保持本轮工作，不每分钟结束对话。仍按原10源含MySQLOracle和Plan全部退出验收；SQLServer真实环境缺口不凑数。

### 2026-10-11 最新人类执行者授权

全程开发仅使用Grok，不调用GPT子智能体；逐源唯一工程师领取不可分割长跑完整能力卡，输入/接口/独占范围/量化指标/开发测试校验与原全部退出一次冻结。GPT子执行器已terminal，后台资源实际收取后交接；旧GPT例外不再用于新开发。集中关键整源独立fixed复审与真实验收保持，降低返工不能承诺零返工或删原条件。

## 2026-10-12 最新人类编译与CPU授权

直接人类要求解除全部编译槽和CPU限制。旧Rustglobal10/task1、CPU16预留总额、builder --cpus1、CARGO_BUILD_JOBS=1及按固定槽排队规定已被覆盖。后续构建不设置固定并行jobs/CPU配额/编译槽上限，使用当前机器可用CPU与内存；独占源码/固定archives、源只读预算、真实测试及权限边界保持。容器CPU按实际cgroup无quota验证；正在运行的编译不中断，旧进程启动时jobs环境只能在下一构建调整，不把文档变更冒充运行参数已改变。可靠Grok目标15与普通clock.sleep900秒保持，worker遇旧资源条文以本最新人类授权为准；依据当前真实机器状态安排，不按旧固定CPU/槽阈值阻塞。

### 2026-10-12 人类收敛与十源并行纠正

十源全部维持原完整产品签收目标，MySQL与Oracle重点，并非仅推进PostgreSQL；每源唯一Grok工程师持续负责原完整链。优先真实可运行产品联结与必要实际测试；一般组件代码质量、非致命审阅及非阻断小问题记录后延后，集中完整产品验收。运行阻断、数据错误、越权/泄密、伪造信任/租约或不真实接受仍必须修复。原真实更新删除和周期收敛、S0–S10标准Yaml/Ontology维护、实际KnowledgePack、Gateway/OOCLI和独立Agent真实多轮出口不削减；源/开发/集成/发行/外部门槛仍分开。可靠并发目标15，全程Grok；编译/CPU固定上限已解除，普通等待clock.sleep900秒。SQLServer原完整源任务保留，真实原生amd64实例/EULA/授权端点缺口不伪造，待条件具备恢复其唯一源工程师。当前整源0/10、受控preview6/10，没有整版签收。

### 2026-10-12 最新直接人类：原生GPT执行与统一闭环审阅

Grok额度不足，后续开发切换Codex原生子智能体，统一gpt-6.1-sol/low，禁止新增Grok CLI。原15个CLI均已终态、已核0个原UUID进程，具名后台计算/数据库保留交接。十源完整产品链全部继续，MySQL/Oracle重点；每源唯一自主工程师，输入/接口/独占写域/真实测试与量化全部退出明确，无独立组件审阅子任务。仅首轮Goal完整闭环及十源完整导入/Selfbuild/同步/Ontology维护/Pack/外部接口/Gateway/实际对话具备后统一独立审阅，普通小问题延后；作者自验与实际证据不可省、未跑不能通过。对话Agent模型gpt-6.1-sol/low，现有代理URL和key不变，密钥仍私有引用。新增Oracle47.104.150.9:1521/scm5仅真实只读选表，复用用户测试secret私有文件，连接身份按实际验证，不枚举秘密。普通clock.sleep900秒、可靠并发15、编译/CPU固定上限解除保持。验收计数不变，整源0/10，Goal保持active。
