# DL01-W1-74：共享 C4 调度、ACK 与中断恢复完整交付

整源工作暂停；本卡是原74共享能力续接，不增加任务或分母。按PONYTAIL最小完整修复，不做框架、额外模型调用或独立组件审阅。工程师 gpt-6-sol/low（临时人类备用授权）。

唯一修改范围：tools/job-worker；evidence/d101fd4a-3a89-41f6-b7fe-94b4001d5512/DL01/capture-jobs-schedule-lease。pipeline/jobs/core/Cargo根与源生产器只读，不改总台账。不接管别人的容器或未明资源。

固定依赖：79源码037f57851c7c3061c94a6c722f9f3fcbea0c9907；Linux1.94 ELF /tmp/oo-dl01w1-79-linux-target/release/oo-pipeline SHA f2d540c87ece921dec032bcc0f87ce111601a61994228d0176f254a28f64b4ea。原worker78349f90f57ae800579aa61d7108091d1250648f，原失败和后台证据保留。参考真实分离DB/ACK输入见 evidence/939f621d-12d2-414f-a355-47be5a34ff71/DL01/native-source-product-loader/raw/c4-post-cas-ack-20261012/。仅使用已具名可交接参考，额外隔离测试实例使用自身prefix。

79命令 ack-native-partition 的stdin为closed JSON：protocol=oo.pipeline.native-partition-ack/v1，source_id/logical_schema/logical_table/partition_id/run_id/batch_id/payload_digest/job_task_id/job_generation；严格从真实Loader成功receipt提取，不手填成功。operator-pinned OO_NATIVE_CONTROL_TARGET及_SHA256/OO_NATIVE_ACK_TARGET及_SHA256指定真实control与目标owner配置，请求不选端点。私有配置引用 /tmp/oo-dl01w1-79-c4-private/control-target.json、ack-target.json，仅用引用和SHA，不写密码或客户数据。与79核字段/运行路径，不从目录名猜版本或身份。

必须完成：生产native-step在真实control jobs完成CAS事务commit后调用固定Loader ACK；持久记录未ACK工作，使CAS后/ACK前中断重启可补ACK，不依赖重新claim succeeded任务，不重COPY、不重复producer或advance watermark。在ACK未完成时不得将完整流水线标成功或放下一分区。保持已存在C1/C2/C3/C5路径、不可变occurrence/批准loader override/heartbeat/权限/哈希检查。ACK结果校验必须精确，不能把child exit0当acknowledged=true。调用时exec清空环境，显式传受信operator固定配置；不把来源请求里的任意环境、路径传进去。

退出：Linux/MSRV测试及固定ELF；真实不同control/target数据库生产worker调用；一例CAS成功后ACK前中断恢复；重复ACK幂等无重COPY/producer；wrong job generation/尚未完成CAS/不合规endpoint或digest拒绝且目标ACK不变；原非C4必要回归。原始stdout/stderr/exit/目标行与sidecar/真实control水位/审计和SHA清单，固定源码+证据提交；完整操作回执及具名资源终态。若依赖缺失明确具体条件，不编假成功/lease，不修改原失败。共享验收不等于十源签收；完成交root及79后停止，root全共享稳定门才释放源。
