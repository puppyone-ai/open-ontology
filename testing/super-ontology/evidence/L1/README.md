# L1 规格与评测基线 · 签收证据

签收版本：OO `feat/super-ontology` @ 92011a799141（签收提交的父提交）；Puppyone `codex/super-drive` @ 530ed9b7f8a9（本地，未推送）。
本机：macOS arm64；Rust 1.98.1（rust-toolchain）；Node 24.13.0（Puppyone 测试；CI 用 Node 22）。

## 30 §L1 验收项

| # | 验收项 | 结果 | 证据 |
|---|---|---|---|
| 1 | flag 关闭时两仓现有测试全绿、行为与基线无差异 | 通过 | OO：签收时 `cargo test --workspace --locked` 2027 通过 0 失败（L1 首次实测 1753），fmt/clippy/docs/contracts/architecture/secrets 全绿；Puppyone：`npm test` 6227/6227（Node 24；L1 首次实测 5688/5688），lint、typecheck、check:boundaries 全绿；T1 flag-off：super-ontology IPC 通道 0、preload 键与基线一致、导航只有原有项（`t1/flag-off-report.json`、`t1/flag-off.png`） |
| 2 | `oo-eval docs` 对评测集出固定格式报告 | 通过 | 空预测全 0（`eval-empty-predictions.json`），标注即预测全 1（`eval-labels-as-predictions.json`） |
| 3 | 打开 flag，侧栏出现 Super Drive，页面显示“引擎未连接” | 通过 | T1 `flag-on-entry.smoke.mjs --native`（macOS CGEvent 真实点击，经 Terminal 启动）：`nativeInput=true`、显示“引擎未连接”与 D79 数据本地说明（`t1/flag-on-native-report.json`、`t1/flag-on-native.png`） |
| 4 | 评测集与 spec 私有词 0 命中 | 通过 | `check_private_terms.py testing/super-ontology contracts crates/oo-docs ...` 0 命中；每个提交前逐文件 grep 0 命中 |

## 30 §4 横切门禁（L1 引入）

| 门禁 | 结果 |
|---|---|
| 私有词 0 命中 | 本地通过；CI 不可用（pr.yml 禁止引用 secrets），作为已知限制 |
| flag 关闭无差异 | 见验收 1 |
| Puppyone check:boundaries | 通过 |
| trailer 与台账一致性 | `check_trailers.py` 0 违规；`check_ledger.py`（含 Puppyone 侧）0 违规；CI 作业 `super-ontology-gates` |

## 会合点

控制 API schema 冻结为 `docs-api/1.0-draft`（`contracts/docs-api/`），Puppyone 已 vendored 并生成类型与 mock（`SOURCE.json` 记录 commit）。

## 形式化（80 §8 L1）

见 `formal.md`。

## 已知问题

- cargo-deny 三条 unmaintained 公告（chrono、chrono-tz、humantime）按 `supply-chain/advisory-exceptions.json` 有期限例外，2027-02-15 到期。
- 指针文档草稿在 `execution/super-ontology/puppy-issues-pointer/`，随 Puppyone 最终 PR 发布。
- 6 种语言（es、pt-BR、fr、de、ja、ko）为初译，待复核。
