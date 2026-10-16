# MySQL MAINLINE REPORT

2026-10-16T13:11:02.961911+00:00

MySQL 5.7.44-log 与 8.4.10 已在隔离固定主线归档中完成正式 `oo-connectors::mysql` 读取、真实 C1 baseline/更新/删除、Loader、CAS 后 ACK、维护 YAML/Ontology/Pack、发布、物理只读 Gateway 与 Rust CLI。最终两新 scope 各 110019 行、5 表、43 列，全量 typed0；8 轮随机 QA 全部 verified，事实逐项绑定实际 OO query_id 及独立 WAL。原两条持续链也恢复至相同真实源端最终数据且 typed0/ready，四个 Rust serve 正常 quiet。

固定证据根：`/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1`。本报告记录作者验证，不替代 root 独立接收、正式 signoff、生产验收或全 Goal 退出；其他源保持暂停，正式源计数不晋级。


## 实现与依赖

`crates/oo-connectors/src/mysql/` 分为 source、reader、map、values、version；lib.rs 只条件导出。MySQL 专属驱动和直接依赖受可选 `mysql` feature 控制，PG 默认不拉 mysql_async。正式库不依赖 tools 或 oo-pipeline；Loader/COPY/目标 DDL 留在适配层。preview 显式启用 feature，C1 baseline/reconcile、native key/source row 实际使用该公共实现，非 re-export 工具包装。

批流按调用者选表列、主键 keyset 分块，保留 Arrow/精确类型、NULL/零日期/signed TIME、TLS 身份校验、源只读授权检查、预算及取消/KILL 清理。实际调用每批上限 1000 行；公开上限/字节/总行/时限见 reader.rs。FULL binlog 行像门禁保留；同 GTID 同表同 key 保留最终事件，真实同键双 UPDATE 已在最终隔离链验证。

## 固定身份

- 基线 `0b27b409a6eb7accc9b382143b3bdbd197444541`；owned 源码最终 `b60bd6fb52c7b021c655bd91ee003d88c84ac204`，既有架构漏声明独立修复 ad28d31439589ee1f44317b0a8fe0cce4798f728；foreign WIP 未采入。
- 固定 archive `/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/mainline-source-b60bd6fb52c7-v4.tar`，SHA256 `26359b01b6bfd6db4e11bf57d1d607e8fb1e853868337c7fcb986f978ca50376`；23 个 owned delta 的逐 Git 字节 SHA 在 mainline-source-provenance-v4.json。
- 归档还含唯一非Git生成物 `tools/mysql-preview/cursor.json`，219字节，SHA256 `ddcb8996776d012315ee634aa08046dc8c7df6a951271d21b6dd70ecbcfe8b8b`。固定单测在新空cwd实跑逐字节复现0；不参与编译或真实产品输入。原archive保留，声明见 generated-cursor-provenance-v1.json；准确口径为23源码delta精确Git字节，另含此声明生成物。
- producer SHA256 `f9aabcdfe4b51146bfe27dabd3e7bb4c3586836d3fd4534352ce20e592b80ec8`；最终运行器 `00f198ff2d1adf287aff7103ab93b1b760969d2f340fc8ead0c6eeb0ac0831f0`。Linux Rust 1.94.0、locked/offline，从独立只读归档重建。
- 新 scope 使用已核固定 release API 95a39e59…d45ce4 / coordinator 882bfa3e…555a9 与 Gateway 520bd301…8364b；真实来源/完整 SHA/源链见 dependencies 与 shared-release-dependency-intake-v1.json。原两 API/issuer 保持原身份，未冒充同一依赖。

| scope | source / generation | GTID end | Pack | serve PID | UTC deadline |
|---|---|---|---|---|---|
| mysql57 | `mysql_48d47e1bc2688ce4ea9bfccf` / 4 | 100 | `44de92bcebec51beac953a97b8cf914e92ed4b06ab98bdf3408700ad5a61fa6f` | 92800 | 2026-10-17T12:59:46.322038+00:00 |
| mysql8 | `mysql_41e1122d356cca616281931c` / 4 | 46 | `68c69e44035cf30344eea2180d12af93ca69532b34cc090b9e6d0c8e9a72c418` | 92824 | 2026-10-17T12:59:46.478634+00:00 |
| original-mysql57 | `mysql_911437f094f3278dab47b513` / 9 | 100 | `88224546576062ee13679011e6e8e1e0d125dadcdf1145d01af3c196607d1e73` | 92812 | 2026-10-17T12:59:46.400570+00:00 |
| original-mysql8 | `mysql_67fff4e8ebff9f91bee8c75e` / 13 | 46 | `242c82b50b2c47360495545924f7fd8bd6c845804bac3b31e38b9d74ebada0b6` | 92836 | 2026-10-17T12:59:46.561252+00:00 |


## 验证与消费

Linux 默认库 73 测试、mysql feature 87 测试、preview 40 passed/1 ignored；最终运行器 9 测试、clippy/release 均0。Clippy 成功为证据根的 native-runtime-current-gateway-clippy-v2.{started.json,stderr,exit}：rust:1.94.0容器安装clippy组件后locked/offline实际exit0；v1缺组件exit1保留，并非host替代。架构原0b baseline exit1 已留，精确 gateway→builder 既有纯编译边同步后，正例/禁止 connectors/pipeline 负例/恢复及既有测试均0。PG 默认 dependency tree 不含 MySQL；mysql-enabled tree 与正式消费者构建均0。

reader-live-v3 实际两源 Arrow 与全量 typed 值比较、错误 CA/主机拒绝、预算/取消后的 backend0 均过。final-producer-core-baseline-live-v1.json 记录最终 f9 实际新绑定冷 baseline：115 批、110024 行、每批≤1000、stage 先于 RR snapshot、正式 reader 名称及 CAS/ACK；不是 quiet 重启冒 baseline。最终删除后原/新四 cut 的 typed0、正式 ACK、发布及 CLI 原始退出在对应 final-serve-v1/generation-*。

最终两新 scope g4 的随机 QA 共8轮、8个源端 named facts/实际原 QID，四个 dialogue verdict 均0，同版本两组去重，失败未重抽。独立冻结 source gold 前后 GTID 不变。已测产品付费为55个唯一 response、137055 tokens：4次必要 S5 15986，最终 QA 51 responses/121069；Grok fallback0。工程师总 usage 未知，未声称达成1/5预算目标。历史原 QA 119437 tokens 未算入本轮。

## 限制与复核

初始新 C1 g1 的 source_committed_at 全 NULL，Gateway 正确拒绝 freshness，不能宣称静态新库一键正常全链。最终成功绑定真实更新后 g4，不填造时间、不削弱门禁。细节及所有实际失败见 FINDINGS。

只读复核：`python3 /tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/MAINLINE-VERIFY.py --out /tmp/NEW_PRIVATE_MAINLINE_VERIFY`。脚本 SHA256 `c7a74829fc3ed07cb5214b8a8eeae71e15c98ac9c8f7f6bc697d6a2dec1041c1`；0模型、0DML、只写全新私有输出。作者已在 /tmp/oo-mysql-mainline-independent-verifier-author-v1 实跑 exit0。完整包位置/manifest/归档 SHA 见最终私有 DELIVERY.json；全局安装仍由 root 接手。
