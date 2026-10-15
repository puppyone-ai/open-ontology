# MySQL 真实共享参考链

最新人类要求：共享接口先在一个真实源完整验通并固定制品，其余源复用；参考源为 MySQL。十源目标不变，当前只放行本参考链所必需的共享实现和 MySQL 真实集成，其余源保持现场、不得发起新的集成或付费模型调用。固定接口不能由每源分别试错定义。

## 输入与范围

复用已授权自有 MySQL 8 ROW/FULL/GTID 源、五表43列实际资料、私有observer账号及TLS；客户RDS不写。原C5目标/旧gen6或10不是C1基线。真实源独立金标与历史GTID输入仅按实际身份使用，不能将旧GTID17或过时行集当当前初始状态。

原依据：docs/04-pipeline.md §4 CDC-first（L0早于快照、CDC先写、快照只补无变更key、墓碑阻止复活、L_end追平后可见）；原MySQL docs C1；Spec首次awaiting_publish不可见/角色Pack发布后Agent准入；主Plan导入/捕获/维护/发布/Gateway原退出。

## 按序退出

1. 固定接口：逐表schema/config、真实L0/L_end和读屏障、快照Arrow、完整有序跨表事务/删除、逐表正式准入registry。capture_digest不包含由该digest派生的registry引用，worker仍独立核完整引用与文件SHA，禁止循环摘要/手填epoch。
2. 固定Linux Rust1.94源码副本编译与必要正拒测试；正式jobs lease下注册真实source instance并取得epoch。真实目标完成CDC-first初始基线/事务原子应用/墓碑与L_end屏障，按原ACK顺序推进。恢复/replay/错误输入不能漏行、复活删除、推进未完成水位。
3. 同一固定版本在MySQL做真实可配置自主两组I/U/D、quiet、恢复；源与目标逐表全量typed oracle差0，协议/质量真实，禁止C5改标C1。
4. 普通六字段+私有secret引用一键进入真实S0–S10；当前标准Yaml/Ontology与角色Pack准确，后续新数据/捕获质量维护不能重写旧S5/伪缓存。
5. 通过正式审核/发布/权限/fence/CAS/只读副本屏障，使当前native导入和批准Pack可由同身份Gateway/OOCLI实际读取。仅本地fixture发布验收，不冒充客户业务签字、生产发行或原生平台认证。
6. 模型前同token/role/currentPack工程师只读预检；真实Agent自主CLI任意合法查询、两个新随机语义/独立源金标/QID与完整WAL、权限拒绝和取消，保留所有失败。工程师预工具答案不进入Agent。
7. 固定源码commit、迁移checksum、源码archive、LinuxELF、接口/运行配置与证据SHA、真实退出与资源receipt。root逐条核全部原退出后，shared-readiness-gate方可release，再让其余九源按原捕获方法复用同一固定制品。参考MySQL通过不等于其余源自动通过。

当前状态：未放行。已确认pipeline host cargo check通过、OOS字段兼容Linux1.94两测试通过；C1摘要循环修复host admission2/worker3测试通过，固定Linux1.94副本admission2/worker3测试通过。实际`import-native-c1-baseline` importer/CLI尚缺，真实初始导入、注册/同步、当前发布与完整对话均未证，不能将协议测试称共享稳定。
