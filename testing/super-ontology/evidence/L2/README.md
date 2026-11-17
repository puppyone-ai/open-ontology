# L2 行走骨架 · OO 侧签收证据（D80 阶段一）

签收版本：OO `feat/super-ontology` @ 17397490110e。Desktop 会合（界面重连、新鲜度顶栏、D79 说明的界面呈现、T1/T2）按 30 §7 在阶段二补齐；L2 的 Desktop T1（真实引擎 15 项）已在 `codex/super-drive` 跑通，作为阶段二的先行证据。

## 30 §L2 验收（OO 侧口径，30 §7.2）

| # | 验收项 | 结果 | 证据 |
|---|---|---|---|
| 1 | 登记含 20 个 Markdown 的 Drive，60 秒内全部 `published` | 通过：3.72 s（子智能体）；签收复跑见 `result-coordinator-signoff.json` | `a1-file-states.json`、`result.json` |
| 2 | `oo context mount --scope Projects/a`，BRIEF 每行带 `[src:…]` | 通过 | `a2-BRIEF.md`、`a2-mount-listing.txt` |
| 3 | 同一输入两次 mount 逐字节相同（除 SHARE.json 的 share id、时间与 refs） | 通过 | `a3-hashes.txt`、`a3-SHARE.t1.redacted.json` |
| 4 | 引擎断开后重连，`since` 补齐不丢状态 | 通过（OO 侧：守护进程与引擎重启后 `since` 回放无缺口且到达 head） | `a4-replay.json` |
| 5 | 修改一个 Markdown，30 秒内状态重跑、BRIEF 变化 | 通过：3.79 s | `a5-state-chain.json`、`a5-BRIEF-after-edit.md` |
| 6 | 新鲜度在 up_to_date / processing / publishing 间切换并带本体版本 | 通过（API 与 `drive.freshness_changed` 事件；七态全集见 L3） | `tests/freshness_live.rs`、L3 脚本 |
| 7 | 默认状态下除 loopback 外无出网 | 通过：serve 与 engine 的全部 socket 均为 127.0.0.1 | `a7-sockets.json` |

## 横切门禁（L2 起，OO 侧）

- Pack 逐字节可重放、BRIEF 出处覆盖率 100%：验收 2、3；L4 门禁另有跨库重放与出处解析测试。
- 形式化（80 §8 L2）：见 `formal.md`（FM1、FM2、FM4 无反例；FM1/FM4 一致性；KP1、KP2、KP8）。
- 签收前全量回归：fmt、clippy、workspace 2109 个测试、architecture、契约、docs、全部 `--ignored` 容器测试套件、L2 脚本 40/40，均为绿。

## 已知问题与阶段二事项

- Desktop 侧：T2 打包黑盒、`--native` 真实点击复跑、D79 说明的界面验证。
- `oo serve` 默认仍为宿主进程模式；compose 运行时已可用（41/41），默认切换在 L3/L4 决定。
