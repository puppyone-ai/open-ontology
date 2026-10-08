/**
 * Public binding registry. OpenAPI for the product groups is absent.
 * GET /v1/pack/metadata is schema_frozen against the gateway source blobs below.
 * Five control-identity operations are fixture_verified against a destroyed local test issuer.
 * The contract commit stays 089b00d7f. Nothing here calls fetch, and live stays false.
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
  openApi: string;
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

export const CONTROL_IDENTITY_COMMIT = "089b00d7f70da1bc42f7e201dfa070748c651d5b";
export const CONTROL_IDENTITY_SERVICE_COMMIT = "35bd6b8157cecd89f2ae085b012b4b0137562ddb";
export const CONTROL_IDENTITY_FIXTURE_RECEIPT = {
  kind: "test_issuer",
  customerIntegration: "not_claimed",
  live: false,
  journey: false,
  release: false,
  serviceCommit: CONTROL_IDENTITY_SERVICE_COMMIT,
  contractCommit: CONTROL_IDENTITY_COMMIT,
  evidence: "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M6-b01-session/raw/browser-receipt.json",
  provedSubject: "fixture-admin",
  provedProject: "alpha",
  provedRole: "admin",
} as const;
export const CONTROL_IDENTITY_OPENAPI = "tools/control-identity/openapi/control-identity.openapi.yaml";

/** Git blob ids of the frozen contract. The working tree matched this commit when pinned. */
export const CONTROL_IDENTITY_BLOBS = {
  "tools/control-identity/CONTRACT.md": "e6752453758114d9408569674b38e8bfeb999bcb",
  "tools/control-identity/openapi/control-identity.openapi.yaml": "3fdeef6b0d22a3be9646520f82275edcb2e3f037",
  "tools/control-identity/schema/current.schema.json": "189ea2499037408ea3ff8693406a698699a5e551",
  "tools/control-identity/schema/logout.schema.json": "120db0995972a2570c302e7d2f82e11009668e9b",
  "tools/control-identity/schema/bootstrap-consume.schema.json": "3ef727d8eca68be03603dd7945e0cbb8eac9f674",
  "tools/control-identity/schema/bootstrap-result.schema.json": "5b43963c1e5e62cb2afc45d9eb4199865de226bb",
  "tools/control-identity/schema/error.schema.json": "7703135c0ab53bf3f8d3ffd789760755eb94d5d9",
} as const;

export const SELFBUILD_COMMIT = "7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9";
export const SELFBUILD_OPENAPI = "tools/selfbuild-control-api/openapi/selfbuild-control-api.openapi.yaml";

/** Git blob ids of the frozen B04 contract. The working tree matched this commit when pinned. */
export const SELFBUILD_BLOBS = {
  "tools/selfbuild-control-api/CONTRACT.md": "8b149c46f1be207bb08e558b0c9abb5347a347c8",
  "tools/selfbuild-control-api/openapi/selfbuild-control-api.openapi.yaml": "8f94655da2d765ed0de75d002e6ff7b736c44b32",
  "tools/selfbuild-control-api/schema/build.schema.json": "350d09161728ed0dee16d513bf7d6a84c7c2c84b",
  "tools/selfbuild-control-api/schema/create-build.request.schema.json": "5fb4d8a04a9b7cc087c103eb705e2d54dee72c0a",
  "tools/selfbuild-control-api/schema/error.schema.json": "92a331049ca3b1a009125089acb27eed96c8312f",
  "tools/selfbuild-control-api/schema/capability-map.json": "b02eff865c600b118bcda272be76ccdf9db215b4",
} as const;

export const SELFBUILD_SCHEMA_SHA256 = {
  openapi: "4a7a155d801e47590f3c60927abf6035da4fc2ca118d318f7f8204330f149cf0",
  build: "1943d0ca62d2bc6ee93aba53cb4e1b9f818e5a7a8bd93e26bc35aa5fd8a35acd",
  createBuildRequest: "7514429cde7d6d2442fa0efd369ca69586e1d330d801af47fc18f8330f76d7da",
  error: "ab989b7de7c402f3bfe44a08e87053b296901ec172914ebc8d2a48518efd08fe",
  capabilityMap: "553fbeda604d502eb4773bc7d8d7eda1d6f3645bb86a1374d8474abacf4f48fb",
} as const;

export const SELFBUILD_HTTP = [
  ["createBuild", "POST", "/control/v1/builds", "build"],
  ["listBuilds", "GET", "/control/v1/builds", "openapi"],
  ["getBuild", "GET", "/control/v1/builds/{build_id}", "build"],
  ["cancelBuild", "POST", "/control/v1/builds/{build_id}/cancel", "build"],
  ["getBuildCards", "GET", "/control/v1/builds/{build_id}/cards", "openapi"],
  ["getBuildCandidates", "GET", "/control/v1/builds/{build_id}/candidates", "openapi"],
  ["listReviewItems", "GET", "/control/v1/reviews", "openapi"],
  ["getReviewItem", "GET", "/control/v1/reviews/{item_id}", "openapi"],
  ["previewReviewBatch", "POST", "/control/v1/reviews/preview", "openapi"],
  ["decideReviewItem", "POST", "/control/v1/reviews/{item_id}/decision", "openapi"],
  ["listClassifications", "GET", "/control/v1/classifications", "openapi"],
  ["previewClassifications", "POST", "/control/v1/classifications/preview", "openapi"],
  ["confirmClassifications", "POST", "/control/v1/classifications/confirm", "openapi"],
  ["compileBuildPack", "POST", "/control/v1/builds/{build_id}/pack", "openapi"],
  ["getBuildPack", "GET", "/control/v1/builds/{build_id}/pack", "openapi"],
] as const;

export const CONTROL_IDENTITY_SCHEMA_SHA256 = {
  openapi: "d49bec8cd3ef59bce78e8f8123fd8ea58be7bedac6d84108a13a2c197bd0f040",
  current: "ffd32c86bd4f5188fb08adcbba49d46231c9cadaaf8530031b5055922e7823cb",
  logout: "3c66428d1c325d45d8d96fcde5e8d1ac9cd53531eecf7c4932f2e6744cfc2064",
  bootstrapConsume: "dcbbe3c79f86b14e621c17c340a470011763d4c8ee1698c5fc59f21759831697",
  bootstrapResult: "42da8ab161dbf673d748b9e9c519c2af2ad75892c2bdc41f89f7bda407748e0d",
  error: "1a6725385580d9d1ec668f84f71d6c441a05fcbe54d07b88046bfdaf98d26238",
} as const;

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

const SELFBUILD_NOTE =
  "schema_frozen。B04 合同提交 7bb2da3cc 已冻结。这里不发送请求，不生成 CSRF，也不把钉住的身份 JSON 当成登录成功。live、journey、release 都为 false。占位的 B04.* 操作仍是 unbound。";

function selfbuild(
  operationId: string,
  method: "GET" | "POST",
  path: string,
  schemaKey: "build" | "openapi",
): BindingRecord {
  return {
    operationId,
    group: "selfbuild",
    method,
    path,
    state: "schema_frozen",
    live: false,
    journey: false,
    release: false,
    openApi: SELFBUILD_OPENAPI,
    presentInProcess: false,
    serviceCommit: SELFBUILD_COMMIT,
    sourceBlobs: SELFBUILD_BLOBS,
    schemaSha256: schemaKey === "build" ? SELFBUILD_SCHEMA_SHA256.build : SELFBUILD_SCHEMA_SHA256.openapi,
    capability: "构建",
    revision: null,
    publicFixture: null,
    note: SELFBUILD_NOTE,
  };
}

function controlIdentity(
  operationId: string,
  method: "GET" | "POST",
  path: string,
  schemaSha256: string,
  note: string,
): BindingRecord {
  return {
    operationId,
    group: "control-identity",
    method,
    path,
    state: "fixture_verified",
    live: false,
    journey: false,
    release: false,
    openApi: CONTROL_IDENTITY_OPENAPI,
    presentInProcess: false,
    serviceCommit: CONTROL_IDENTITY_SERVICE_COMMIT,
    sourceBlobs: CONTROL_IDENTITY_BLOBS,
    schemaSha256,
    capability: "身份",
    revision: null,
    publicFixture: null,
    note,
  };
}

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

const CONTROL_IDENTITY_NOTE =
  "fixture_verified。本地测试发行方，不是客户 IdP。服务提交 35bd6b8157cecd89f2ae085b012b4b0137562ddb，合同提交 089b00d7f70da1bc42f7e201dfa070748c651d5b。页响应在 browser-receipt.json：login 302、callback 302、current 200、consume 200、logout 200。同一测试断言错误 CSRF 的 logout 为 403，且会话未被撤销。客户集成 not_claimed。live、journey、release 都为 false。运行时 origin 是 null。本模块不发送请求。占位的 B01.* 仍是 unbound。";

export const OPERATIONS: readonly BindingRecord[] = [
  metadata,
  controlIdentity(
    "startAuthorizationCodeLogin",
    "GET",
    "/control/v1/auth/login",
    CONTROL_IDENTITY_SCHEMA_SHA256.openapi,
    `${CONTROL_IDENTITY_NOTE} 响应是 302，没有成功 JSON。schemaSha256 是 OpenAPI 文档。`,
  ),
  controlIdentity(
    "completeAuthorizationCodeCallback",
    "GET",
    "/control/v1/auth/callback",
    CONTROL_IDENTITY_SCHEMA_SHA256.openapi,
    `${CONTROL_IDENTITY_NOTE} 响应是 302，没有成功 JSON。schemaSha256 是 OpenAPI 文档。`,
  ),
  controlIdentity(
    "getIdentityCurrent",
    "GET",
    "/control/v1/identity/current",
    CONTROL_IDENTITY_SCHEMA_SHA256.current,
    `${CONTROL_IDENTITY_NOTE} schemaSha256 是 current.schema.json。没有身份夹具。`,
  ),
  controlIdentity(
    "logoutManagementSession",
    "POST",
    "/control/v1/auth/logout",
    CONTROL_IDENTITY_SCHEMA_SHA256.logout,
    `${CONTROL_IDENTITY_NOTE} schemaSha256 是 logout.schema.json。需要 CSRF 与 Origin，本模块不生成它们。`,
  ),
  controlIdentity(
    "consumeBootstrapToken",
    "POST",
    "/control/v1/auth/bootstrap/consume",
    CONTROL_IDENTITY_SCHEMA_SHA256.bootstrapResult,
    `${CONTROL_IDENTITY_NOTE} schemaSha256 是响应 bootstrap-result.schema.json。请求体是 bootstrap-consume.schema.json，哈希 ${CONTROL_IDENTITY_SCHEMA_SHA256.bootstrapConsume}。`,
  ),
  ...SELFBUILD_HTTP.map(([operationId, method, path, schemaKey]) => selfbuild(operationId, method, path, schemaKey)),
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
