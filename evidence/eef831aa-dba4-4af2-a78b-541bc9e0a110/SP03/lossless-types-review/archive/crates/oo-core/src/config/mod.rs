//! Deployment, source, file, role, agent, hook, and eval configuration.
//!
//! `schema_version` is required on every document. Narrative samples in the
//! configuration reference omit it; those samples are not valid documents until
//! `config/v1` is added. Cross-field rules live in [`validate`]. Schema shape
//! and serde accept the same fields.

pub mod de;
pub mod model;
pub mod parse;
pub mod validate;
pub mod version;

pub use model::{
    AgentConfig, Binding, BindingMatch, Budgets, Channels, ClaimValue, ColumnExclude, Connection,
    ConsistencyMode, DateSystem, Deployment, EvalSuite, FeishuChannel, FileMode, FileSet,
    FilesFile, Grant, GroupPolicy, Harness, Hook, HookParams, HookType, HooksFile, Incremental,
    LlmForBuilder, ModelConfig, ObjectBackend, ObjectStore, OoConfig, ParseProfile,
    ParseProfileInline, PiiLevel, ReadFrom, RejectsAlert, Retention, Role, RoleDefaults,
    RoleLimitOverride, RoleLimits, RolesFile, SampleValues, Sandbox, SandboxDriver, SandboxPool,
    SandboxResources, Scoring, SecretBackend, Secrets, Session, SheetMapping, SlackChannel,
    SnapshotMode, Source, SourceLimits, SourcesFile, SyncMode, TableGrant, TableOverride,
    TableSelect, Tinyint1Policy, TlsMode, TypePolicy, Visibility, WebChannel, ZeroDatePolicy,
    effective_freshness,
};
pub use parse::{
    parse_agent_yaml, parse_eval_suite_yaml, parse_files_yaml, parse_hooks_yaml, parse_oo_yaml,
    parse_roles_yaml, parse_sources_yaml,
};
pub use validate::effective_role_limits;
pub use version::{ConfigVersion, require_config_version};
