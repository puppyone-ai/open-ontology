# Rust Runtime Handoff

作者完整 fixture 证据交 root，代码 `e3e83b3995c19305de9bc8530c08b0fcc9722b9e`（基础 `1afe10bb624cc4d26c53339bfc6e06d9d9606d9b`）；原 Plan/分母与正式计数不变。没有组件 reviewer 子任务、没有子派，也没有新增 Grok。

当前安全 cut：
- 5.7：`/tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/mysql57-linux-reentry-v10b/generation-5/`
- 8.4：`/tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/mysql8-linux-reentry-v10b/generation-8/`
- 全量 typed oracle、current OOS/YAML、maintenance、native request/load/ACK、release、Gateway-ready 均在各 cut 内。
- 四组真实 GPT QA 在 `mysql57-dialogue1-v8`、`mysql57-dialogue2-v8`、`mysql8-dialogue1-v8`、`mysql8-dialogue2-v8`；原 WAL 与 gold 不改。v10b 按同 Pack/current gold 复用，当前 v10b caller/cancel 与 Secret 隔离另做实测。

当前长跑由同一 operator 容器 `oo-mysql-rust-runtime-linux-v8` 承载，容器名沿用 v8，实际两版运行 **v10b ELF** `b0808d42b885366d7825aef1ea266d3120a614180dd8e943ba58a57263e84d68`，每个 binding 仅一 writer；实际 namespace PID 分别 50451 / 50463，周期 1000ms，duration 86400s。

退出期限的实测窄窗（UTC，capture 完成与首次 fire marker 之间锚定）：
- 5.7：['2026-10-17T08:38:26.568234+00:00', '2026-10-17T08:38:26.568628+00:00']
- 8.4：['2026-10-17T08:38:26.675912+00:00', '2026-10-17T08:38:26.676269+00:00']

时钟跳变会影响该绝对时间估计。长任务仍 running，raw exit 是 pending，未伪造 0；wrapper 到真实退出才写 `.exit`。`retained-linux-resources-verified-v10b.json` 记录 PID/argv/实际 `/proc/PID/exe` SHA、quiet、期限锚点、host client PID 和资源。

client token 到期：5.7 `2026-10-16T09:34:24.713000+00:00`；8.4 `2026-10-16T09:34:20.895000+00:00`。无自动续签。官方 issuer 的新文件续签方式见 OPERATIONS；不要覆写原 token、QA 或 pins。

独立复核：
```sh
python3 /tmp/oo-mysql-rust-runtime-sol-medium-20261016/verify-rust-runtime-v10b.py --out /tmp/oo-mysql-root-independent-NEW
```
SHA256 `317a6ac501266ca4b0ae7e52e662472c3df0a8b03a7921a1c3057daebd0d77a4`。目录必须不存在；只生成新输出。脚本真实查询当前 GTID、复核八事实/原 QID WAL、当前 Pack、current caller、OOS 和 admin/source Secret 不可读探针。root 先读脚本再执行，不需要付费模型或 DML。原 private verifier v1/v8 与失败输出均保留。

root 待办：独立核固定包；决定默认入口/registry 切换和原 legacy Gateway 治理；保持 fixture 与正式签收分层。旧 `onboard.py` recipe 仍是历史注册，未冒充已改成新 Rust 默认入口。若未核实本包，不据此放行其他源。
