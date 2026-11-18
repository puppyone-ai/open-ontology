# L8 规模与轻量 · OO 侧证据（D80 阶段一，**未签收**）

状态：**未签收**。原因：与 L4 相同的 `conformance_share` 偶发失败（向量开启的全量运行里失败 1 次，单独重跑通过）；`download` 与 L3 一样被阻塞（S3 镜像不在本机）。**L8 自身的验收项全部通过。** OO `feat/super-ontology` @ 4390b2cb4。独立复核运行于 `e1242d241`（验证分支 `so/V8-signoff`，2026-11-18，验证员与执行者分离），结果 `result-verify-signoff.json`：**该次记录的状态是 fail**：10 项检查中 9 项通过，1 项失败（向量开启时 `speaker_live` 的 D63 检查被 0144 新增的 `block_vector.embedding` 命中，首次与复跑都失败），`e877ac64d` 把该检查改为仍拒绝声纹与嵌入列、但允许块文本向量表。之后在 4cee27056 上复跑向量开启的全部 53 个 ignored 目标（`vectors_on_runs`，`oo-docs-pgvector:dev`，`OO_DOCS_VECTORS=hashgram/1`）：首次 51 通过、1 失败（`conformance_share`）、1 跳过（`download`，S3 镜像不在本机）；`conformance_share` 单独重跑通过。因此 `speaker_live` 在向量下已通过，剩下的偶发失败与阻塞就是未签收的原因。Desktop 部分（1 万文件首屏、虚拟滚动）按 30 §7 在阶段二补齐。

## 30 §L8 验收（OO 侧口径，30 §7.2）

| # | 验收项 | 结果 | 证据 |
|---|---|---|---|
| 1 | 1 万文件 Drive 首屏 < 1 秒、虚拟滚动无掉帧 | OO 侧通过：1 万文件列举加 stat 0.184 s（上限 60 s），文件列表 API p95 26.2 ms（上限 100 ms）。**首屏渲染与虚拟滚动不掉帧为阶段二（Desktop），未做** | `result-verify-signoff.json`、`bench-scale10k.md` |
| 2 | 冷启动建 Index 的时间与内存在 10 分册上限内 | 通过（release 构建的合成 Markdown，10000 文件、25100 条事实）：建 Index 352.4 s（上限 18000 s），发布 86.2 s，engine RSS 186 MiB（上限 1536 MiB），docs-db 采样峰值 216 MiB。限定：scale 路径只断言 docs-db 内存 > 0，**没有断言 PG ≤ 1 GiB**（V2 审计；该容器在此路径不受限）；发布时间相对 1000 文件（L4 基准 1.6 s）是超线性的，在上限内，未优化；数据为合成小 Markdown，不代表真实文件体积 | `result-verify-signoff.json`、`bench-scale10k.json`（执行者，363 s / 87 s） |
| 3 | 无 Docker 的 macOS 机器从安装到第一份 BRIEF ≤ 10 分钟 | 暖缓存下通过：ADR-S14 脚本（md、docx、pdf 共 22 个文件，sidecar 在 `sandbox-exec` 下）从脚本启动到首份 BRIEF 49.08 s（独立复核）；执行者的两次原型 36.5 s、6.9 s（纯 Markdown，debug 构建）。**前提与未测部分**：uv 缓存已暖（18 GB），模型从本地镜像复制，PostgreSQL 为已装好的 Homebrew 17，使用 debug 构建；**冷下载（uv 冷安装：镜像源 16 s，默认 PyPI 约 100 KB/s，torch 35 分钟未完成）、Hugging Face 模型下载、应用与二进制的下载或构建都没有测，是 owner 事项**。ADR-S14 状态仍是 Proposed（单独立项评审未完成） | `result-verify-signoff.json`、`nodocker.json`、`nodocker-sidecar.json`、`docs/adr/ADR-S14-docker-free-runtime.md` |
| S13 | 可选向量混合检索，不破坏确定性（只用于 BRIEF“其他相关”与 resolve 召回，结果固定版本、参与排序键） | 部分验证：安装套件在普通 PG 17.11 与 pgvector 镜像上各 9/9；`OO_DOCS_VECTORS=hashgram/1` 下 `gates_live` 3/3；e1242d241 上向量打开时其余 ignored 套件 65 个通过、1 个失败（`speaker_live` 的 D63 检查，测试已在 `e877ac64d` 修正）；4cee27056 上向量打开的 53 个目标 51 通过、`conformance_share` 失败 1 次（单独重跑通过）、`download` 跳过。没有单独的“仅 Pack 重放”测试，Pack 与门禁重放由 `gates_live`、`conformance`、`api_live` 在全量运行里覆盖 | `result-verify-signoff.json` |

## 横切门禁与许可（L8，OO 侧）

- SBOM 许可门禁（D64）：自建最小 pgvector 镜像（官方 `postgres:17.11-bookworm` 加 pgvector 0.8.0）`SBOM gate: PASS`（144 个基础包、3 个镜像内组件）。**先前使用的 `supabase/postgres` 镜像门禁 FAIL（38 项 FAIL、36 项需审阅、13 项未知许可），已被拒绝，仅留作记录。**
- **owner 事项：D67 的法务门禁——重新分发该 pgvector 基础镜像（含 bash、coreutils 等 GPL 的 Debian 包）的法律审阅尚未做**；门禁只列出而不检查 144 个基础包。另有未完成项：发布构建的镜像推送与摘要固定、与 TLS 入口的 compose 接线。该镜像仅在打开向量时使用，docs-db 默认仍是官方镜像。
- 形式化：L8 在 D81 下没有新的形式化项，快速门禁沿用 L4 的完整运行，见 `formal.md`。
- L2、L4 脚本在该 HEAD 上通过：`super_ontology_l2.py` 40/40，`super_ontology_l4.py` 70/70。

## 已知缺口

- 原型中发现的问题（ADR-S14）：macOS 上 sidecar 默认 `forkserver` worker 在 import torch 后会段错误（信号 11），用 `spawn` 可行但每次请求约多 8 s；PyPI 的 `opencv-python-headless` 轮子自带 FFmpeg，D65 禁止随发行版分发，所以发行版需要自编不含 FFmpeg 的 OpenCV（10 至 25 分钟）或在无表格结构的情况下处理 PDF；`sandbox-exec` 已弃用，自测只检查所列的拒绝项，不是安全证明；macOS 锁文件尚未生成（现有 lock 的哈希是 linux 轮子）。
- 以上测量全部在一台 Mac（arm64）上，且 oo 与 engine 是 debug 构建。
- 本证据里没有向量对 BRIEF“其他相关”与 resolve 召回效果的评测数字，只验证了安装、门禁与功能。
