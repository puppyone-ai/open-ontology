# Rust Runtime Findings

1. 必须补的是 whole/follow 中真实接入调度、维护和发布职责。原 Rust driver/Loader/API/Pack/release/Gateway/CLI、合法 S5 与完整历史故障证据继续有效。没有新增 DSL、服务、通用 recipe 框架或九源实现。
2. 原服务进程不存在，但控制库留两过期 running task。正式 worker 原任务重入后 generation=2、complete=nochange；这是租约恢复证据，不能写成新的 IUD crash 测试。
3. v7 published-cut 重入曾把 source-wide schema_digest 与逐表 catalog schema_digest 错误等同，真实拒绝并保留 raw exit；修正为原契约的各自身份校验后 v8/v10b 实跑通过，没有改旧收据或放松目标身份检查。
4. 最终安全复核发现继承旧 Follow 的 admin 文件进入 Gateway bundle。修正为控制容器私有引用签发、仅复制 client token；实际挂载与不可读探针通过。自己的中间 v4/v8 Gateway 已停止；原 legacy/49 pins 由 root 处理，不宣称全局轮换或清理已完成。
5. 本轮最初 smoke 有 archive prefix 路径错误与所选 Cargo cache 缺 mysql_async，均保留失败 stdout/stderr/exit；随后用独占 fixed source、复制的已有 registry 与必要 index 补齐通过。Luna 交接中的四次未进测试失败及缺 raw exit 仍是历史缺口，不改写成通过。
6. 所有付费问答先过无模型 preflight。八轮全部原随机 selection、原事实和 QID 通过；安全修正后只在严格同 source/generation/Pack/gold 下复用，无重新抽题、新 S5 或 provider 探测。
7. 尚不属于本交付：生产/customer owner 签署、任意新客户环境、正式源计数、全 Goal、默认 registry 切换、性能/吞吐认证、自动 token 续签及 boot daemon。工程 token 的 1/5 指标没有可比计量，保持未知。

8. 最终补回原 published-cut 的 pg_attribute OID/typmod 实查，不能仅靠行值同值代替类型正确；第一次实测因 OID 在 JSON 中输出文本而 fail closed（v10 raw 保留），查询显式 cast bigint 后按原整数 OID/typmod 精确比对。最终六项测试及两版 live attrs/full-cell/维护/RO 全部通过，无新模型。
