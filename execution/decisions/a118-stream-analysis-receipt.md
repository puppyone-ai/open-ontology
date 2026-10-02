# a118 流式读取分析接收与实施约束

分析提交：`325a6a0f5d4f92efe24e11e7037e4e3f2b779e99`；tree `ed5b5175686ea8884b1be99bf423fc70d815bf86`；parent `5e33c1e37fc2a7d94b36185b545ff7c3ca65034b`。实际 CLI handle 61827 已 exit 0，关闭，不再轮询。完整 REPORT/FINDINGS/HANDOFF 已分段阅读，三文件 HASHES.txt 校验通过。

接收为设计输入，不接收为实现或独立验收。无 builder、数据库或测试执行；R1、R2–R4 和完整 reader 均未因此通过。产品指标和验收勾选不变。

## 已确认的问题

`PgStream::recv_unchecked` 在 DataRow 解码和业务 charge 前请求整个消息的 reserve；fetch_all、LIMIT 和后置字节检查不能给出分配上限。字段、整行、控制消息、解码副本、Arrow 数组必须分别计量。底层实际 capacity 可超过 reserve 参数，必须记录实际高水位。

## 对提案的必要修正

1. **协议边界**：Execute 请求多行后，不能因 batch_bytes 达限而假定已到 PortalSuspended。必须准确区分尚未消费的请求行与同步边界；不得丢行、提前 Sync 或把局部页当 EOF。先以 Execute 一行验证状态机，后续优化须证明相同语义。CommandComplete/Close/Sync 顺序也须用真实协议测试验证。
2. **连接准入**：在 connect 前获取源级连接 permit，并在所有失败、Drop 和释放路径归还。read_run 才检查数量无法阻止第五条已建立连接，不是连接预算验收。稳定源身份、并发竞态和拒绝前零连接需测。
3. **速率预算**：在受控查询/Execute 前预约预算，完成后核算；禁止把 open 已抓取的行或已输出数据事后扣账当成执行限速。控制查询、重试和失败请求必须有明确计量规则。
4. **全路径上限**：pager 有界不等于 open/catalog/probe 有界。后续独立任务必须把接收上限接入全部生产源读取路径，并界定 catalog 的聚合预算。保留旧接口默认行为可以用于兼容，但不能作为生产安全完成证明。
5. **完整扫描准入**：全量扫描不应因为没有主键、主键类型超出 probe 支持或键宽超过八而被错误拒绝。主键限制属于 probe/range API；全模式允许无键且不合成键。无可保真列的 presence 扫描须单独明确计数与 schema 语义。
6. **内存证明**：明确 Int32 长度含自身四字节而 payload 不含；测真实 allocator capacity、累计字段、跨帧 overread、逻辑值/JSON 树/数组/克隆/Arrow 的高水位。消费者自行保留多个批次必须明示，不能宣称整个进程仅一个批次。
7. **释放与成功语义**：中途错误/丢弃不能产生 Finished、TableEof 或完整 run EOF。R3 的取消和后台退出证明仍是独立验收要求；close_hard 不等同已证明 backend 消失。

## 可独立开发顺序

- R2-W：驱动接收上限与实际分配计量；独立提交和 Linux/MSRV 冒烟，固定提交复审。
- R2-P：命名 portal 协议状态机，真实暂停/继续/完成/失败同步测试。
- R2-S：同一 snapshot 的全量多表 pull/Arrow 流，完整扫描准入、批次预算、预算前置与慢消费者测试。
- R3/R4：取消、后台退出、完整 cut 成功与后续 pipeline 接入，保留原计划边界。

R2-W prepared 卡不激活 UUID 或 builder。当前 slot A 为 a116，slot B 为 a119；两任务实际终态并接收证据后再调度。禁止第三个 builder。无 root/manifest/lock writer。
