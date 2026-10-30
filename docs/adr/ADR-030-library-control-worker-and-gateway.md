# ADR-030：库级控制 worker 与单 Gateway 的受控落地

状态：匿名多来源夹具已实现；真实库验收单独记录，不由本 ADR 推定。

## 决策

复用现有 Self-build build、stage_checkpoint、overlay、review-ledger 与发布控制面，
不新增调度或发布系统。可信项目配置增加 `library`：Python 解释器、阶段入口及摘要、
库配置及摘要、LIB manifest 及摘要。HTTP `createBuild` 只引用安装侧登记项目，不能上传
可执行命令、认证信息或自行提高预算。项目注册仍是安装配置，不新增 project CRUD。

worker 通过既有受控子进程调用 `tools/pg-builder/scripts/library_control.py`：S0 读取
当前 Agent DB 元数据/离线 DDL；S1–S4 共享一次全局确定性 facts；S5 每批至多 16 表，
失败最多重试两次且每次失败后冷却至少 65 秒；S6–S8 合并到一个 generated overlay。
S9/S10 使用已认证管理身份、逐对象 ledger 决定与分类确认。分批确认不会提前完成 S10；
隐式旧绑定不能复用，显式 CAS 更新采用 enqueue 返回的新 revision。人工 curated 不被覆盖。

编译仍调用同一个 Rust role-pack compiler，只产出一个角色 Pack。库级 envelope 为
`oo.selfbuild.library-pack/v1`，始终 `business_owner_signed=false`、`published=false`；
release API 的库级 intake 重算编译产物、输入摘要和输出摘要，再由原 coordinator stage、
gates、CAS、fence 与 LIB 原子发布路径执行，不接受手写 ready 或发布记录。

安装容量与请求权限分离：审核 scope 默认 64、控制文件默认 64 KiB；大型安装显式提供
有界 limits，仍逐对象精确授权，不接受通配符。编译输入/输出、文件数、表数、宽表列数
须协调配置。Gateway 默认 Pack 与 catalog 上限保留；库级安装显式提供 ceilings。
容量不会增加可见列，也不放开 HAVING、聚合排序、子查询、CASE 等 SQL 类别。

Gateway 的 `oo.gateway.library-reviewed/v1` 从已编译 Pack、effective OOS、visibility、
闭合库 receipt 和实际导入 binding vector 建立同一角色策略。它不是虚构的源端捕获 seal。
每次 catalog 加载核对当前 run/generation、实际映射及目标类型、库 head、全部成员就绪、
目标 fence 和 opened_pack_digest；业务查询仍使用只读登录与 NOLOGIN 数据角色。

freshness 将库级 data readiness 与源提交时间分开：每表提供 target_observations，保留
source_ids、CROSS_TABLE_SKEW；源提交时间/共同源位点未测量则显式 unmeasured。
不能因没有声明源引擎而把有完整库发布/导入证据的健康数据误标 STALE_DATA；缺失或关闭
库成员、未封口 run、未耗尽导入或坏目标时间仍拒绝就绪。不会把目标应用时间填入源 lag。

## 质量与限制

强制 RLS 可能过滤发布管理员的观察。安装侧可配置与 target 完全相同端点/数据库的
`quality_pg`，只执行平台生成的元数据/聚合 SQL，并在服务器确认 READ ONLY；RLS 过滤
不能当作空表。此凭据不进入 Gateway。独立关系包含度观察只证明目标值匹配，不认证
业务含义；fixture/test 审核和未解释码仍明确标注，不能升级为业务 owner 签字或生产证书。

匿名验收覆盖 72 表、六来源、502 列宽表、70 条关系、真实 Sonnet 分批、ledger、发布及
Linux Gateway/CLI。大型真实库、完整 glossary 审核、评测和运维清理由独立里程碑留证，
不从夹具通过推导完成。逐文件 Unicode-word 估计不是模型 tokenizer 计数。

## 运行更新：并行模型阶段

后续真实整库运行改用 `grok-4.7`，旧模型证据仅作历史，不能冒充新模型缓存。
`s5_concurrency` 默认 8、上限 32；每批独立重试/冷却，结果按批序返回。强关联与
每表最多五条实测正包含度候选进入 S5；空表/未测量弱候选仍保留在全局 facts。
超大 enum 以引用、证据和明确截断的样本进入模型，不删确定性码值。预算保持不变。
可信 S0/facts 复用必须同时验证文件摘要和当前逐表 generation 向量；自动审核守护
记录 fixture/test 决定及阶段耗时，不能把自动执行等同业务 owner 签字。
