# MySQL MAINLINE OPERATIONS

固定证据根：`/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1`。本报告记录作者验证，不替代 root 独立接收、正式 signoff、生产验收或全 Goal 退出；其他源保持暂停，正式源计数不晋级。


最终运行器 `/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/bin/00f198ff2d1adf287aff7103ab93b1b760969d2f340fc8ead0c6eeb0ac0831f0/source-onboarding-linux`，producer `f9aabcdfe4b51146bfe27dabd3e7bb4c3586836d3fd4534352ce20e592b80ec8`。固定源archive与ELF SHA见REPORT，运行器 source commit b60bd6fb52c7b021c655bd91ee003d88c84ac204。所有下列命令在受信 Linux 控制容器 oo-mysql-rust-runtime-linux-v8 内运行；host不安装系统/etc。

| scope | source / generation | GTID end | Pack | serve PID | UTC deadline |
|---|---|---|---|---|---|
| mysql57 | `mysql_48d47e1bc2688ce4ea9bfccf` / 4 | 100 | `44de92bcebec51beac953a97b8cf914e92ed4b06ab98bdf3408700ad5a61fa6f` | 92800 | 2026-10-17T12:59:46.322038+00:00 |
| mysql8 | `mysql_41e1122d356cca616281931c` / 4 | 46 | `68c69e44035cf30344eea2180d12af93ca69532b34cc090b9e6d0c8e9a72c418` | 92824 | 2026-10-17T12:59:46.478634+00:00 |
| original-mysql57 | `mysql_911437f094f3278dab47b513` / 9 | 100 | `88224546576062ee13679011e6e8e1e0d125dadcdf1145d01af3c196607d1e73` | 92812 | 2026-10-17T12:59:46.400570+00:00 |
| original-mysql8 | `mysql_67fff4e8ebff9f91bee8c75e` / 13 | 46 | `242c82b50b2c47360495545924f7fd8bd6c845804bac3b31e38b9d74ebada0b6` | 92836 | 2026-10-17T12:59:46.561252+00:00 |


四条周期1000ms、最长86400s，当前 owner PID 已实核 /proc/PID/exe 等于最终 runtime SHA；期限见表，exit pending。不可并行启动同binding第二个serve，先官方状态/owner/租约核单writer。普通重入使用新的 --out；`after_generation`须由当前正式 native-state 决定，不手改任务/epoch。历史挂起/过期lease按官方claim_selected同task恢复，失败和实际exit保留。

```sh
source-onboarding-linux check --input SIX_FIELDS.json --operator PRIVATE_OPERATOR.json --out NEW_PRIVATE_CHECK
source-onboarding-linux capture --input SIX_FIELDS.json --operator PRIVATE_OPERATOR.json --out NEW_PRIVATE_CAPTURE
source-onboarding-linux follow --input SIX_FIELDS.json --operator PRIVATE_OPERATOR.json --out NEW_PRIVATE_FOLLOW
source-onboarding-linux serve --input SIX_FIELDS.json --operator PRIVATE_OPERATOR.json --out NEW_PRIVATE_SERVE
```

这四份 operator 是同一已部署fixture的真实入口，不代表未准备客户环境。首次新客户仍需官方安装/凭据/TLS/授权/选择表/发布和身份前置。初始baseline时间未知时freshness会拒绝，不能承诺任意静态库六字段即所有出口通过。辅助fixture/QA检查器可用Python；产品capture/follow/serve直接执行固定Rust业务ELF，不启动Python业务编排。

最终正常长serve operator没有 reuse_published_cut；该6pin分支只用于已发布待GW的合法重入，不能长期固定旧cut。最新ready指针只在完整GW/CLI成功后原子替换；本轮已实证连续下一周期更新、删除，坏指针拒绝。

客户端 grant TTL=3600000ms，长serve期限不等于客户端token期限。必要续签在受信control通过官方issuer写独立0600文件并按当前gold/Pack核验；不把admin/source Secret挂进GW，不篡改原pins。只读verifier不会续签或变更注册，若client过期由root先正式续签再在新out重放。

无固定CPU/jobs cap；独立target oo-mysql-mainline-sol-target，缓存固定Linux1.94。实际观测构建/四源消费期间控制容器约9.389GiB/16GiB，最终quiet采样324.6MiB/16GiB；只是已测样本，非完整峰值承诺。保留已固定cache/ELF/fixture API、DB和physicalstandby；作者未停原root治理legacy GW，未改全局默认registry。

原两fixture producer升级除了native-put，还须官方 native-loader-put 更新已存在override；只更新schedule不能证明实际调用新ELF。本轮 before/after/逐process_attempt SHA在 original-*-feature-override-v1。最终由root安装默认并归档旧Python mysql注册，不能伪造whole receipt。
