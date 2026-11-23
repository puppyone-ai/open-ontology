# Desktop 阶段二 P9 轮：T1 重跑、T2 状态、owner 包（2026-11-18）

范围：L2–L8 的 Desktop 会合证据在各自的 `../L<n>/desktop/README.md`；这里放共用的运行记录。**没有任何闭环的 Desktop 部分被标为已签。**

## 环境

- Puppyone：分支 `sd/P9-evidence`，基于 `codex/super-drive` `f2ddc8cc`；T1 在该树加一处测试改动（s3 场景的 lane 标签取自环境变量，提交 `23ca7546`）上运行。Node v24.13.0，Electron 41.10.6，原生辅助模块（拖拽、模板、媒体解码）在本机构建。
- OO：`feat/super-ontology` `fad4f5651`。`cargo build --locked -p oo-cli --features docs`、`cargo build --locked -p oo-docs --features objectstore-s3 --bin oo-docs-engine`（`CARGO_BUILD_JOBS=6`）。二进制 sha256 见 `packs.md`。
- 容器：标签 `oo.lane=super-ontology-p9`（Postgres 用 `SUPER_ONTOLOGY_LANE_LABEL`，compose 栈用 `OO_DOCS_LANE`），同一时刻至多 2 个、顺序运行；运行后查询该标签的容器数为 0。
- 环境变量：`SUPER_ONTOLOGY_OO_BIN`、`_ENGINE_BIN`、`_GOLDEN`（OO 评测集）、`_MODEL_EGRESS`、`_RECORDED_LLM`（`crates/oo-docs/tests/fixtures/llm`）、`_FIXTURES`、`_S3=1`、`_COMPOSE=1`。模型一律是回放，没有真实密钥或出网。

## T1 结果（每个场景各跑一次；`run-summary.txt`、`logs/`、`reports/`）

| 场景 | 引擎 | 结果 |
|---|---|---|
| skeleton/walking-skeleton | 真实 | 通过（15 项） |
| flags/flag-off-regression、flags/flag-on-entry | 真实 Main | 通过 |
| engine/engine-states | 假 docker + 假 oo | 通过 |
| visual/state-matrix | 假 | 通过 |
| processing/details-anchor、processing/formats-and-failures | 真实 | 通过 |
| sync/s3-incremental | 真实 + RustFS | **失败**（缺陷 D1，登记即 400） |
| processing/parser-outage | 真实 compose | **失败**（缺陷 D2，没有“降级”信号） |
| share/mount-and-revoke、review/accept-by-source、model/configure-and-extract、drift/refresh | 真实（回放模型） | 通过 |
| suggest/revision-mode | 假 | 通过 |
| media/queue-and-decode | 真实，无 media-parser | 通过（只 1 项；codec 卡未走到） |
| enterprise/buckets、enterprise/cloud-proxy | 假 | 通过 |
| scale/ten-thousand | 假 | 首次失败（p95 17.0 ms），重跑 2 次通过 |
| runtime/docker-free | 假运行时 | 通过 |

`--native`：未运行成功。`native-flag-on-entry.log`：Timed out waiting for native window focus（终端没有辅助功能权限）。所以 20 §7 要求“含 `--native`”的 T1 完成定义**都没有满足**。

## T2

`peekaboo permissions`：Screen Recording、Accessibility 均 Not Granted（Event Synthesizing 可选，同样 Not Granted）。`run-packaged.mjs --dry-run`（`t2-dry-run.log`）：26 个场景、32 个断言校验通过。60 §5 要求的 **P12、P19、P20、P26 没有场景文件**。T2 一个也没有运行。

## 其他门禁

`npx vitest run tests/unit/super-drive tests/architecture/super-drive`：44 个文件 540 条通过（`logs/vitest-super-drive.log`）。`npm run check:boundaries`：通过，末项 `Super Drive contract OK: docs-api 1.9`（`logs/check-boundaries.log`）。

## 发现的缺陷（交给开发池；本轮没有改 OO 代码）

- **D1** OO engine 与契约不一致：`contracts/docs-api` 的 S3 配置是 `access_key_id`/`secret_access_key` 两个 `SecretRef`，`crates/oo-docs/src/api/drives.rs` 的 `s3_config` 只允许单个 `secret_ref`（`only_keys` 拒绝其余键）。桌面端按契约发，真实引擎 400；oss/cos/r2 同形。
- **D2** `crates/oo-docs/src/health.rs`（`parser` 组件恒为 ok）：解析边车被停时 `/health` 不降级，OO 里也没有“解析器不可用”的错误码。L3 验收 2 的 Desktop 呈现因此无源。
- **D3** 一万文件滚动帧间隔在有负载时会越过 16.7 ms，未稳定。
- **D4** 打包后的验收应用在构建机的沙箱 shell 里启动后空转（未查明，可能与环境有关）。
- **D5** Puppyone 缺 P12、P19、P20、P26 的 T2 场景；没有“导入示例文件夹”按钮，示例也没有作为 `extraResources` 打进验收包（60 §3.4）；`oo` 未随 App 打包。
