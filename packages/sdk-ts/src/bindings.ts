/**
 * Public binding registry. OpenAPI for the product groups is absent.
 * GET /v1/pack/metadata is schema_frozen against the gateway source blobs below.
 * Nothing in this module calls fetch, and live stays false.
 */

export type BindingState = "unbound" | "schema_frozen" | "fixture_verified" | "live_verified";

export type BindingRecord = {
  operationId: string;
  group: string;
  method: "GET" | "POST" | null;
  path: string | null;
  state: BindingState;
  live: false;
  journey: false;
  release: false;
  openApi: "absent";
  presentInProcess: boolean;
  serviceCommit: string | null;
  sourceBlobs: Readonly<Record<string, string>> | null;
  schemaSha256: string | null;
  capability: string | null;
  revision: null;
  publicFixture: string | null;
  note: string;
};

export type InvokeResult = {
  ok: false;
  reason: "unknown_operation" | "not_connected" | "schema_only";
  operationId: string;
};

export const GATEWAY_SOURCE = {
  observedHead: "d22187ea0fce8e420323660841f2da3b447d5de8",
  packMetadataCommit: "97b6a9eb9caa2b6059d82bdb98b703bf5ee202d8",
  errorSchemaCommit: "3f4a358ab1502215e956cdfe1f02ba899b06a8ca",
  protocolCommit: "6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c",
  blobs: {
    "crates/oo-gateway/src/http_gateway.rs": "969e655d7458d8b679933ffb6f361f08741cabea",
    "crates/oo-gateway/src/http_error.rs": "482621857488168e85b78a11bec17855fd7649e6",
    "crates/oo-gateway/src/pack_catalog.rs": "9848dcafc4c36b6512240f20fec1b09b1146c02a",
  },
} as const;

export const PACK_METADATA_SCHEMA_SHA256 = "f77d55d45bfe86e237f167881a415f67304e9dc11251474c4155a3ac2a9353e3";
export const GATEWAY_ERROR_SCHEMA_SHA256 = "0839b3e4798ece2ceaddee80ae2c71d54fd491f919d2025616884b97cbc8b5f2";

const NOT_CONNECTED = "OpenAPI 不存在，没有冻结的 HTTP schema，不能调用。";

function unbound(group: string, name: string, capability: string): BindingRecord {
  return {
    operationId: `${group}.${name}`,
    group,
    method: null,
    path: null,
    state: "unbound",
    live: false,
    journey: false,
    release: false,
    openApi: "absent",
    presentInProcess: false,
    serviceCommit: null,
    sourceBlobs: null,
    schemaSha256: null,
    capability,
    revision: null,
    publicFixture: null,
    note: NOT_CONNECTED,
  };
}

function presentRoute(
  operationId: string,
  method: "GET" | "POST",
  path: string,
  note: string,
): BindingRecord {
  return {
    operationId,
    group: "gateway",
    method,
    path,
    state: "unbound",
    live: false,
    journey: false,
    release: false,
    openApi: "absent",
    presentInProcess: true,
    serviceCommit: GATEWAY_SOURCE.packMetadataCommit,
    sourceBlobs: GATEWAY_SOURCE.blobs,
    schemaSha256: null,
    capability: null,
    revision: null,
    publicFixture: null,
    note,
  };
}

const GROUP_OPERATIONS: readonly (readonly [string, string, string])[] = [
  ["B01", "oidc", "身份"],
  ["B01", "bootstrap", "初始化"],
  ["B01", "identity", "身份"],
  ["B01", "capability", "能力"],
  ["B01", "projectRoleBinding", "项目角色绑定"],
  ["B02", "connectorMatrix", "连接器"],
  ["B02", "sourceConfig", "数据源"],
  ["B02", "inspect", "体检"],
  ["B02", "catalog", "目录"],
  ["B02", "dba", "DBA 清单"],
  ["B02", "plan", "计划"],
  ["B02", "apply", "应用"],
  ["B03", "jobs", "任务"],
  ["B03", "checkpoints", "检查点"],
  ["B03", "events", "事件"],
  ["B03", "cancel", "取消"],
  ["B03", "retry", "重试"],
  ["B04", "builder", "构建"],
  ["B04", "cards", "候选卡"],
  ["B04", "candidates", "候选"],
  ["B04", "reviews", "审核"],
  ["B04", "classifications", "分类"],
  ["B05", "terms", "术语"],
  ["B05", "codeTables", "码表"],
  ["B05", "calendar", "日历"],
  ["B05", "metric", "指标"],
  ["B05", "entity", "实体"],
  ["B05", "examples", "示例"],
  ["B05", "pack", "角色包"],
  ["B05", "gates", "门禁"],
  ["B05", "publish", "发布"],
  ["B06", "roles", "角色"],
  ["B06", "bindings", "绑定"],
  ["B06", "permissionPreview", "权限预览"],
  ["B06", "clientToken", "客户端令牌"],
  ["B06", "revoke", "撤销"],
  ["B06", "oauth", "OAuth"],
  ["B07", "secretWrite", "Secret 写入"],
  ["B07", "secretRotate", "Secret 轮换"],
  ["B07", "objectPart", "对象分片"],
  ["B07", "objectComplete", "对象完成"],
  ["B07", "objectAbort", "对象中止"],
  ["B07", "download", "下载"],
  ["B07", "objectDelete", "对象删除"],
  ["B08", "conversation", "对话"],
  ["B08", "session", "会话"],
  ["B08", "turn", "轮次"],
  ["B08", "event", "事件"],
  ["B08", "history", "历史"],
  ["B08", "restore", "恢复"],
  ["B08", "close", "关闭"],
  ["B09", "tables", "表"],
  ["B09", "describe", "描述"],
  ["B09", "query", "查询"],
  ["B09", "validate", "校验"],
  ["B09", "entities", "实体"],
  ["B09", "values", "取值"],
  ["B09", "freshness", "新鲜度"],
  ["B09", "knowledge", "知识"],
  ["B09", "export", "导出"],
  ["B09", "gap", "缺口"],
  ["B09", "receipt", "回执"],
  ["B10", "suites", "评测集"],
  ["B10", "run", "评测运行"],
  ["B10", "result", "评测结果"],
  ["B10", "privateHoldout", "保留集"],
  ["B10", "compare", "评测比较"],
  ["B11", "git", "Git"],
  ["B11", "config", "配置"],
  ["B11", "models", "模型"],
  ["B11", "harness", "harness"],
  ["B11", "channels", "渠道"],
  ["B11", "deployment", "部署"],
  ["B11", "plugin", "插件"],
  ["B12", "health", "健康"],
  ["B12", "audit", "审计"],
  ["B12", "replay", "回放"],
  ["B12", "emergency", "紧急停止"],
  ["B12", "backup", "备份"],
  ["B12", "upgrade", "升级"],
  ["B12", "recovery", "恢复"],
];

const metadata: BindingRecord = {
  operationId: "gateway.packMetadata.get",
  group: "gateway",
  method: "GET",
  path: "/v1/pack/metadata",
  state: "schema_frozen",
  live: false,
  journey: false,
  release: false,
  openApi: "absent",
  presentInProcess: true,
  serviceCommit: GATEWAY_SOURCE.packMetadataCommit,
  sourceBlobs: GATEWAY_SOURCE.blobs,
  schemaSha256: PACK_METADATA_SCHEMA_SHA256,
  capability: null,
  revision: null,
  publicFixture: "packages/sdk-ts/src/fixtures/pack-metadata.example.json",
  note: "schema_frozen。响应形状来自 pack_body，错误信封来自 ApiError::body。示例夹具不是服务器捕获，fixture_verified 与 live 都为 false。这不是 B01 身份，也不是 B09 查询。",
};

export const OPERATIONS: readonly BindingRecord[] = [
  metadata,
  presentRoute(
    "gateway.sql.post",
    "POST",
    "/v1/sql",
    "进程对这条路由返回 501 SQL_UNSUPPORTED。它不是 B09 查询，不绑定为可用查询。",
  ),
  presentRoute(
    "gateway.health.get",
    "GET",
    "/health",
    "进程里有这条路由。本里程碑不绑定它，也不把它当成 B12 运维健康。",
  ),
  presentRoute(
    "gateway.ready.get",
    "GET",
    "/ready",
    "进程里有这条路由。本里程碑不绑定它。",
  ),
  presentRoute(
    "gateway.session.current.get",
    "GET",
    "/v1/session/current",
    "进程里有这条路由。它不是 B01 产品身份，本里程碑不绑定。",
  ),
  presentRoute(
    "gateway.probe.slow.get",
    "GET",
    "/slow",
    "只在 probe 进程注册。不是产品操作，不绑定。",
  ),
  presentRoute(
    "gateway.probe.hold.get",
    "GET",
    "/hold",
    "只在 probe 进程注册。不是产品操作，不绑定。",
  ),
  ...GROUP_OPERATIONS.map(([group, name, capability]) => unbound(group, name, capability)),
];

export function bindingById(operationId: string): BindingRecord | null {
  return OPERATIONS.find((item) => item.operationId === operationId) ?? null;
}

export function operationsByGroup(group: string): readonly BindingRecord[] {
  return OPERATIONS.filter((item) => item.group === group);
}

/** Schema and route records are not a transport. This never calls fetch. */
export function invokeOperation(operationId: string): InvokeResult {
  const operation = bindingById(operationId);
  if (!operation) {
    return { ok: false, reason: "unknown_operation", operationId };
  }
  if (operation.state === "schema_frozen") {
    return { ok: false, reason: "schema_only", operationId };
  }
  return { ok: false, reason: "not_connected", operationId };
}
