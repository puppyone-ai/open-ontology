//! SQL admission for one top-level `SELECT`.
//!
//! The caller supplies the project, role, and policy identity plus the visible
//! relations, columns, and already-resolved function and operator OIDs. This
//! module parses with `pg_query` (libpg_query), rewrites stars and logical
//! names on that AST, and deparses. A bounded literal `IN` list is admitted
//! without rewriting the predicate into `OR` or `AND`. It does not
//! authenticate, resolve OIDs from names, connect to PostgreSQL, or execute SQL.
//!
//! Names the snapshot does not list are omitted from errors. Parse failures
//! drop the parser text. `pg_query::protobuf::ParseResult::nodes` is not the
//! completeness check; the walker below visits the owned protobuf fields.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use sha2::{Digest, Sha256};

use pg_query::protobuf as pb;
use postgres_types::Type;

use oo_core::api::ErrorCode;

/// Maximum SQL text accepted by [`guard_select`], in bytes.
pub const SQL_BYTE_BUDGET: usize = 64 * 1024;

/// Maximum protobuf `Node` values visited from the statement.
pub const NODE_BUDGET: usize = 10_000;

/// Maximum `Node` nesting depth. The statement node is depth 1.
pub const DEPTH_BUDGET: usize = 128;

/// Maximum explicit elements in one admitted `IN` or `NOT IN` list.
pub const IN_LIST_LIMIT: usize = 32;

/// `PG_VERSION_NUM` embedded in pg_query 6.2.1. Not a live server probe.
pub const PINNED_PARSER_VERSION: i32 = 170007;

const SETOP_UNDEFINED: i32 = pb::SetOperation::Undefined as i32;
const SETOP_NONE: i32 = pb::SetOperation::SetopNone as i32;
const JOIN_INNER: i32 = pb::JoinType::JoinInner as i32;
const JOIN_LEFT: i32 = pb::JoinType::JoinLeft as i32;
const JOIN_FULL: i32 = pb::JoinType::JoinFull as i32;
const JOIN_RIGHT: i32 = pb::JoinType::JoinRight as i32;
const AEXPR_OP: i32 = pb::AExprKind::AexprOp as i32;
const AEXPR_IN: i32 = pb::AExprKind::AexprIn as i32;
const AEXPR_OP_ANY: i32 = pb::AExprKind::AexprOpAny as i32;
const AEXPR_OP_ALL: i32 = pb::AExprKind::AexprOpAll as i32;
const BOOL_AND: i32 = pb::BoolExprType::AndExpr as i32;
const BOOL_OR: i32 = pb::BoolExprType::OrExpr as i32;
const BOOL_NOT: i32 = pb::BoolExprType::NotExpr as i32;
const NULL_IS: i32 = pb::NullTestType::IsNull as i32;
const NULL_NOT: i32 = pb::NullTestType::IsNotNull as i32;
const SORT_USING: i32 = pb::SortByDir::SortbyUsing as i32;
const LIMIT_TIES: i32 = pb::LimitOption::WithTies as i32;

/// Exact project, role, and policy identity. Compared byte for byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyIdentity {
    /// Project key supplied by the future adapter.
    pub project: String,
    /// Role name the adapter has already bound. Not an authentication check.
    pub role: String,
    /// Policy version or digest the adapter treats as current.
    pub policy_id: String,
}

/// Visible schema snapshot. OIDs are resolved by the adapter, not by this guard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyCatalog {
    /// Must equal the identity passed to [`guard_select`].
    pub identity: PolicyIdentity,
    /// Relations the role may read. A fully hidden table is omitted.
    pub relations: Vec<VisibleRelation>,
    /// Functions the adapter has already resolved to one OID each.
    pub functions: Vec<ResolvedFunction>,
    /// Binary operators the adapter has already resolved.
    pub operators: Vec<ResolvedOperator>,
    /// Type OIDs for integer, text, and boolean literals. Absent means unresolved.
    pub literal_types: LiteralTypes,
    /// `IS NULL` / `IS NOT NULL` on a column. No OID is invented when this is false.
    pub null_test_allowed: bool,
    /// R9 threshold. A starred relation is large when `estimated_rows` is greater.
    pub large_table_row_threshold: u64,
}

/// One relation the role can name in SQL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleRelation {
    /// Logical schema. Schema-qualified SQL uses this name.
    pub schema: String,
    /// Logical relation name.
    pub name: String,
    /// Physical schema from the approved mapping. Often `_oo`.
    pub physical_schema: String,
    /// Physical relation name from the approved mapping.
    pub physical_name: String,
    /// Visible columns in catalog order. Empty is rejected.
    pub columns: Vec<VisibleColumn>,
    /// Hidden columns that still exist. The count may be shown; the names are not.
    pub pending_unclassified: u32,
    /// Adapter-supplied row estimate. Not read from `pg_class` here.
    pub estimated_rows: u64,
}

/// One visible column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleColumn {
    /// Logical column name.
    pub name: String,
    /// Physical column name from the approved mapping.
    pub physical_name: String,
    /// Adapter-supplied type OID. Zero is rejected.
    pub type_oid: u32,
}

/// One function the adapter resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedFunction {
    /// `pg_proc.oid`. Not derived from the name.
    pub oid: u32,
    /// Schema. `pg_catalog` is allowed. Relation schemas are a separate ban.
    pub schema: String,
    /// Function name.
    pub name: String,
    /// Argument type OIDs, in order. Empty for `aggregate_star`.
    pub argument_type_oids: Vec<u32>,
    /// `count(*)`-style call with no argument types.
    pub aggregate_star: bool,
}

/// One binary operator the adapter resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedOperator {
    /// `pg_operator.oid`.
    pub oid: u32,
    /// `pg_operator.oprcode`.
    pub procedure_oid: u32,
    /// Operator schema. `pg_catalog` is allowed.
    pub schema: String,
    /// Operator token, such as `=`.
    pub name: String,
    /// Left operand type OID.
    pub left_type_oid: u32,
    /// Right operand type OID.
    pub right_type_oid: u32,
}

/// Literal type OIDs. `None` leaves that literal unresolved in expressions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LiteralTypes {
    /// Integer literal type, when the adapter has one.
    pub integer: Option<u32>,
    /// Text literal type, when the adapter has one.
    pub text: Option<u32>,
    /// Boolean literal type, when the adapter has one.
    pub boolean: Option<u32>,
}

/// Admitted plan. `executed_sql` is the deparsed AST, never a raw fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardedPlan {
    /// Identity that matched the catalog.
    pub identity: PolicyIdentity,
    /// SQL text the caller passed, unchanged.
    pub original_sql: String,
    /// SQL produced by deparsing the rewritten AST.
    pub executed_sql: String,
    /// SHA-256 of `original_sql` bytes, lowercase hex.
    pub original_sha256: String,
    /// SHA-256 of `executed_sql` bytes, lowercase hex.
    pub executed_sha256: String,
    /// `pg_query` fingerprint of `executed_sql`. Clustering only, not equality.
    pub fingerprint_hex: String,
    /// Parser protobuf version. This pin requires [`PINNED_PARSER_VERSION`].
    pub parser_version: i32,
    /// Stars expanded in the original projection, in select-list order.
    pub star_expansions: Vec<StarExpansion>,
    /// Relations, outputs, functions, and operators recognized on the original SQL.
    pub bindings: RecognizedBindings,
}

/// One star and the logical columns it became.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StarExpansion {
    /// `""` for `*`, an alias or relation for `t.*`, or `schema.name` for `schema.rel.*`.
    pub qualifier: String,
    /// Logical schema.
    pub schema: String,
    /// Logical relation.
    pub name: String,
    /// Logical column names in catalog order.
    pub columns: Vec<String>,
}

/// What the original statement referenced after scope resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecognizedBindings {
    /// `FROM` items in join order.
    pub relations: Vec<BoundRelation>,
    /// Select-list items in output order.
    pub outputs: Vec<OutputItem>,
    /// Function calls, including those outside the select list.
    pub functions: Vec<BoundFunction>,
    /// Operator calls that matched one snapshot OID.
    pub operators: Vec<BoundOperator>,
    /// Admitted `IS NULL` / `IS NOT NULL` tests.
    pub null_tests: u32,
}

/// One `FROM` range after alias resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundRelation {
    /// Logical schema.
    pub schema: String,
    /// Logical relation.
    pub name: String,
    /// SQL alias, when the statement has one.
    pub alias: Option<String>,
    /// Physical schema written into the executed statement.
    pub physical_schema: String,
    /// Physical relation written into the executed statement.
    pub physical_name: String,
}

/// One select-list item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputItem {
    /// A column reference or an expanded star column.
    Column(BoundOutputColumn),
    /// A function call in the select list.
    Function(BoundFunction),
    /// A literal in the select list.
    Literal(LiteralKind),
    /// An expression that is not a single column, function, or literal.
    Compound,
}

/// A column projected by the original statement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundOutputColumn {
    /// Logical relation schema.
    pub relation_schema: String,
    /// Logical relation name.
    pub relation_name: String,
    /// Logical column name.
    pub name: String,
    /// Physical column name in the executed statement.
    pub physical_name: String,
    /// Result column name. User alias, otherwise the logical column name.
    pub output_name: String,
    /// Adapter-supplied type OID.
    pub type_oid: u32,
}

/// A function call that matched one snapshot entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundFunction {
    /// Snapshot OID.
    pub oid: u32,
    /// Snapshot schema.
    pub schema: String,
    /// Snapshot function name.
    pub name: String,
    /// True for `count(*)`-style calls.
    pub aggregate_star: bool,
}

/// An operator call that matched one snapshot entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundOperator {
    /// Snapshot operator OID.
    pub oid: u32,
    /// Snapshot procedure OID.
    pub procedure_oid: u32,
    /// Snapshot schema.
    pub schema: String,
    /// Operator token.
    pub name: String,
}

/// Kind of literal the select list projected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiteralKind {
    /// Integer constant.
    Integer,
    /// String constant.
    Text,
    /// Boolean constant.
    Boolean,
    /// `DATE 'YYYY-MM-DD'` constant. Not a cast of a column or a function.
    Date,
    /// `NULL`.
    Null,
}

/// Admission refusal. Display text does not include hidden names or parser text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuardError {
    /// The argument identity and the catalog identity differ.
    IdentityMismatch,
    /// The snapshot shape is not usable. `reason` is a static label.
    CatalogInvalid {
        /// Static label. No caller-supplied text.
        reason: &'static str,
    },
    /// `pg_query` rejected the text.
    Parse,
    /// Byte length, node count, or depth exceeded the budget.
    Budget,
    /// The statement is not one plain `SELECT`.
    Statement,
    /// A write statement was present.
    Write,
    /// `SELECT INTO`.
    Into,
    /// A locking clause was present.
    Locking,
    /// A schema the role must not name was present.
    SystemSchema,
    /// The construct is outside this slice. `construct` is a static label.
    Unsupported {
        /// Static label. No caller-supplied text.
        construct: &'static str,
    },
    /// Deparse, reparse, or the physical recheck did not succeed.
    Serialize {
        /// Static label. No SQL text.
        reason: &'static str,
    },
    /// No single snapshot function or operator matched.
    FunctionNotAllowed,
    /// The relation is absent or not visible. Those cases use the same error.
    RelationNotFound,
    /// The column is absent, hidden, or ambiguous. The requested name is omitted.
    ColumnNotAllowed {
        /// Visible `alias.column` or `schema.relation.column` labels.
        available: Vec<String>,
        /// Sum of pending columns on the relations named in `available`.
        pending: u32,
        /// More than one in-scope relation exposes the column.
        ambiguous: bool,
    },
    /// A `FROM` relation has no schema.
    Unqualified {
        /// `schema.name` when exactly one visible relation has that name.
        suggestion: Option<String>,
    },
    /// Original star, no real filter, relation above the row threshold.
    CostTooHigh {
        /// Visible `schema.name`.
        relation: String,
    },
}

impl GuardError {
    /// Stable API code. Retry is never indicated for these refusals.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Parse => ErrorCode::SqlParseError,
            Self::FunctionNotAllowed => ErrorCode::FunctionNotAllowed,
            Self::RelationNotFound => ErrorCode::RelationNotFound,
            Self::ColumnNotAllowed { .. } => ErrorCode::ColumnNotAllowed,
            Self::Unqualified { .. } => ErrorCode::UnqualifiedRelation,
            Self::CostTooHigh { .. } => ErrorCode::CostTooHigh,
            Self::IdentityMismatch | Self::CatalogInvalid { .. } => ErrorCode::SchemaInvalid,
            Self::Budget
            | Self::Statement
            | Self::Write
            | Self::Into
            | Self::Locking
            | Self::SystemSchema
            | Self::Unsupported { .. }
            | Self::Serialize { .. } => ErrorCode::StatementNotAllowed,
        }
    }

    /// Actionable hint that does not repeat hidden names.
    pub fn hint(&self) -> Option<String> {
        match self {
            Self::Unqualified {
                suggestion: Some(name),
            } => Some(format!("use {name}")),
            Self::Unqualified { suggestion: None } => Some("use schema.name".to_string()),
            Self::CostTooHigh { .. } => Some("add a filter or use export".to_string()),
            _ => None,
        }
    }
}

impl fmt::Display for GuardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityMismatch => formatter.write_str("policy identity does not match"),
            Self::CatalogInvalid { reason } => {
                write!(formatter, "policy catalog is invalid: {reason}")
            }
            Self::Parse => formatter.write_str("SQL parse failed"),
            Self::Budget => formatter.write_str("statement exceeds the admission budget"),
            Self::Statement => formatter.write_str("statement is not allowed"),
            Self::Write => formatter.write_str("write statement is not allowed"),
            Self::Into => formatter.write_str("SELECT INTO is not allowed"),
            Self::Locking => formatter.write_str("locking clause is not allowed"),
            Self::SystemSchema => formatter.write_str("system schema is not allowed"),
            Self::Unsupported { construct } => {
                write!(
                    formatter,
                    "statement construct is not supported: {construct}"
                )
            }
            Self::Serialize { reason } => {
                write!(formatter, "statement could not be serialized: {reason}")
            }
            Self::FunctionNotAllowed => formatter.write_str("function or operator is not allowed"),
            Self::RelationNotFound => formatter.write_str("relation is not visible"),
            Self::ColumnNotAllowed {
                available,
                pending,
                ambiguous,
            } => write!(
                formatter,
                "{}",
                column_message(available, *pending, *ambiguous)
            ),
            Self::Unqualified { .. } => formatter.write_str("relation must be schema-qualified"),
            Self::CostTooHigh { relation } => {
                write!(formatter, "projection of {relation} has no filter")
            }
        }
    }
}

impl Error for GuardError {}

/// Admit one SQL string. Returns a plan or a refusal. Does not execute SQL.
pub fn guard_select(
    identity: &PolicyIdentity,
    catalog: &PolicyCatalog,
    sql: &str,
) -> Result<GuardedPlan, GuardError> {
    guard_with(identity, catalog, sql, Surface::User, true)
}

fn guard_with(
    identity: &PolicyIdentity,
    catalog: &PolicyCatalog,
    sql: &str,
    surface: Surface,
    recheck: bool,
) -> Result<GuardedPlan, GuardError> {
    validate(identity, catalog, surface)?;
    if sql.len() > SQL_BYTE_BUDGET {
        return Err(GuardError::Budget);
    }
    let mut parsed = pg_query::parse(sql).map_err(|_| GuardError::Parse)?;
    if parsed.protobuf.version != PINNED_PARSER_VERSION {
        return Err(GuardError::Unsupported {
            construct: "parser version",
        });
    }
    let scan = scan_tree(&parsed.protobuf)?;
    if scan.saw_write {
        return Err(GuardError::Write);
    }
    let mut admitted = admit_select(&mut parsed.protobuf, catalog, surface)?;
    if let Some(relation) = large_relation(&admitted, catalog) {
        return Err(GuardError::CostTooHigh { relation });
    }
    let executed = pg_query::deparse(&parsed.protobuf)
        .map_err(|_| GuardError::Serialize { reason: "deparse" })?;
    if executed.is_empty() {
        return Err(GuardError::Serialize { reason: "deparse" });
    }
    let reparsed =
        pg_query::parse(&executed).map_err(|_| GuardError::Serialize { reason: "reparse" })?;
    if reparsed.protobuf.version != PINNED_PARSER_VERSION {
        return Err(GuardError::Serialize { reason: "version" });
    }
    let again = pg_query::deparse(&reparsed.protobuf).map_err(|_| GuardError::Serialize {
        reason: "second deparse",
    })?;
    if again != executed {
        return Err(GuardError::Serialize {
            reason: "idempotent",
        });
    }
    let fingerprint = pg_query::fingerprint(&executed).map_err(|_| GuardError::Serialize {
        reason: "fingerprint",
    })?;
    if recheck {
        let physical = physical_catalog(catalog);
        let second = guard_with(identity, &physical, &executed, Surface::Physical, false)?;
        if second.executed_sql != executed || !second.star_expansions.is_empty() {
            return Err(GuardError::Serialize { reason: "recheck" });
        }
    }
    admitted.relations.sort_by_key(|relation| relation.ordinal);
    let plan = GuardedPlan {
        identity: identity.clone(),
        original_sql: sql.to_string(),
        original_sha256: sha256_hex(sql),
        executed_sha256: sha256_hex(&executed),
        fingerprint_hex: fingerprint.hex,
        parser_version: parsed.protobuf.version,
        star_expansions: admitted.expansions,
        bindings: RecognizedBindings {
            relations: admitted
                .relations
                .into_iter()
                .map(|relation| relation.bound)
                .collect(),
            outputs: admitted.outputs,
            functions: admitted.functions,
            operators: admitted.operators,
            null_tests: admitted.null_tests,
        },
        executed_sql: executed,
    };
    Ok(plan)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Surface {
    User,
    Physical,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fold {
    True,
    False,
    Other,
}

enum WhereFold {
    Absent,
    TrueConst,
    Real,
}

struct Admitted {
    expansions: Vec<StarExpansion>,
    relations: Vec<OrderedRelation>,
    outputs: Vec<OutputItem>,
    functions: Vec<BoundFunction>,
    operators: Vec<BoundOperator>,
    null_tests: u32,
    where_fold: WhereFold,
    starred: Vec<usize>,
}

struct OrderedRelation {
    ordinal: usize,
    bound: BoundRelation,
}

struct RangeBind {
    rel_index: usize,
    alias: Option<String>,
    exposed: String,
    ordinal: usize,
}

struct Ctx<'a> {
    catalog: &'a PolicyCatalog,
    surface: Surface,
    ranges: Vec<RangeBind>,
    expansions: Vec<StarExpansion>,
    outputs: Vec<OutputItem>,
    functions: Vec<BoundFunction>,
    operators: Vec<BoundOperator>,
    null_tests: u32,
    starred: Vec<usize>,
    next_ordinal: usize,
}

enum ExprKind {
    Column {
        rel_index: usize,
        column_index: usize,
        type_oid: u32,
    },
    Literal {
        kind: LiteralKind,
        fold: Fold,
    },
    Function(BoundFunction),
    Operator,
    Bool(Fold),
    NullTest,
}

struct Scan {
    nodes: usize,
    saw_write: bool,
}

impl Scan {
    fn enter(&mut self, depth: usize) -> Result<(), GuardError> {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > NODE_BUDGET || depth > DEPTH_BUDGET {
            Err(GuardError::Budget)
        } else {
            Ok(())
        }
    }
}

fn scan_tree(tree: &pb::ParseResult) -> Result<Scan, GuardError> {
    let mut scan = Scan {
        nodes: 0,
        saw_write: false,
    };
    for raw in &tree.stmts {
        if let Some(node) = &raw.stmt {
            descend_node(node, 1, &mut scan)?;
        }
    }
    Ok(scan)
}

fn admit_select(
    tree: &mut pb::ParseResult,
    catalog: &PolicyCatalog,
    surface: Surface,
) -> Result<Admitted, GuardError> {
    if tree.stmts.len() != 1 {
        return Err(GuardError::Statement);
    }
    let statement = tree.stmts[0].stmt.as_mut().ok_or(GuardError::Statement)?;
    let select = match statement.node.as_mut() {
        Some(pb::node::Node::SelectStmt(select)) => select.as_mut(),
        Some(_) => return Err(GuardError::Statement),
        None => return Err(GuardError::Statement),
    };
    reject_select_shape(select)?;
    let mut ctx = Ctx {
        catalog,
        surface,
        ranges: Vec::new(),
        expansions: Vec::new(),
        outputs: Vec::new(),
        functions: Vec::new(),
        operators: Vec::new(),
        null_tests: 0,
        starred: Vec::new(),
        next_ordinal: 0,
    };
    bind_from_list(&mut select.from_clause, &mut ctx)?;
    rewrite_targets(&mut select.target_list, &mut ctx)?;
    for key in &mut select.group_clause {
        if !matches!(admit_expr(key, &mut ctx)?, ExprKind::Column { .. }) {
            return Err(GuardError::Unsupported {
                construct: "GROUP BY",
            });
        }
    }
    let where_fold = admit_where(select.where_clause.as_mut(), &mut ctx)?;
    admit_sort(&mut select.sort_clause, &mut ctx)?;
    if ctx.ranges.is_empty() && uses_column(&ctx) {
        return Err(GuardError::Unsupported {
            construct: "column reference",
        });
    }
    let relations = ctx
        .ranges
        .iter()
        .map(|range| ordered_relation(range, catalog))
        .collect();
    Ok(Admitted {
        expansions: ctx.expansions,
        relations,
        outputs: ctx.outputs,
        functions: ctx.functions,
        operators: ctx.operators,
        null_tests: ctx.null_tests,
        where_fold,
        starred: ctx.starred,
    })
}

fn uses_column(ctx: &Ctx<'_>) -> bool {
    ctx.outputs
        .iter()
        .any(|item| matches!(item, OutputItem::Column(_)))
        || !ctx.starred.is_empty()
}

fn reject_select_shape(select: &pb::SelectStmt) -> Result<(), GuardError> {
    if select.with_clause.is_some() {
        return Err(GuardError::Unsupported { construct: "CTE" });
    }
    if !select.distinct_clause.is_empty() {
        return Err(GuardError::Unsupported {
            construct: "DISTINCT",
        });
    }
    if select.into_clause.is_some() {
        return Err(GuardError::Into);
    }
    if select.group_distinct {
        return Err(GuardError::Unsupported {
            construct: "GROUP BY",
        });
    }
    if select.having_clause.is_some() {
        return Err(GuardError::Unsupported {
            construct: "HAVING",
        });
    }
    if !select.window_clause.is_empty() {
        return Err(GuardError::Unsupported {
            construct: "WINDOW",
        });
    }
    if !select.values_lists.is_empty() {
        return Err(GuardError::Unsupported {
            construct: "VALUES",
        });
    }
    if !select.locking_clause.is_empty() {
        return Err(GuardError::Locking);
    }
    let plain = matches!(select.op, SETOP_UNDEFINED | SETOP_NONE)
        && select.larg.is_none()
        && select.rarg.is_none()
        && !select.all;
    if !plain {
        return Err(GuardError::Unsupported {
            construct: "set operation",
        });
    }
    if select.limit_option == LIMIT_TIES {
        return Err(GuardError::Unsupported {
            construct: "LIMIT WITH TIES",
        });
    }
    if let Some(limit) = &select.limit_count {
        nonnegative_const(limit)?;
    }
    if let Some(offset) = &select.limit_offset {
        nonnegative_const(offset)?;
    }
    Ok(())
}

fn nonnegative_const(node: &pb::Node) -> Result<(), GuardError> {
    match node.node.as_ref() {
        Some(pb::node::Node::AConst(value)) if !value.isnull => match &value.val {
            Some(pb::a_const::Val::Ival(integer)) if integer.ival >= 0 => Ok(()),
            _ => Err(GuardError::Unsupported { construct: "limit" }),
        },
        _ => Err(GuardError::Unsupported { construct: "limit" }),
    }
}

fn bind_from_list(items: &mut [pb::Node], ctx: &mut Ctx<'_>) -> Result<(), GuardError> {
    for item in items {
        bind_from_item(item, ctx)?;
    }
    Ok(())
}

fn bind_from_item(node: &mut pb::Node, ctx: &mut Ctx<'_>) -> Result<(), GuardError> {
    match node.node.as_mut() {
        Some(pb::node::Node::RangeVar(var)) => bind_range(var, ctx),
        Some(pb::node::Node::JoinExpr(join)) => bind_join(join, ctx),
        Some(pb::node::Node::RangeSubselect(_)) => Err(GuardError::Unsupported {
            construct: "subquery",
        }),
        Some(pb::node::Node::RangeFunction(_)) => Err(GuardError::Unsupported {
            construct: "range function",
        }),
        Some(pb::node::Node::RangeTableFunc(_)) => Err(GuardError::Unsupported {
            construct: "table function",
        }),
        Some(pb::node::Node::RangeTableSample(_)) => Err(GuardError::Unsupported {
            construct: "tablesample",
        }),
        _ => Err(GuardError::Unsupported {
            construct: "from item",
        }),
    }
}

fn bind_join(join: &mut pb::JoinExpr, ctx: &mut Ctx<'_>) -> Result<(), GuardError> {
    if !matches!(
        join.jointype,
        JOIN_INNER | JOIN_LEFT | JOIN_FULL | JOIN_RIGHT
    ) {
        return Err(GuardError::Unsupported {
            construct: "join type",
        });
    }
    if join.is_natural {
        return Err(GuardError::Unsupported {
            construct: "NATURAL JOIN",
        });
    }
    if !join.using_clause.is_empty() || join.join_using_alias.is_some() {
        return Err(GuardError::Unsupported {
            construct: "JOIN USING",
        });
    }
    if join.alias.is_some() {
        return Err(GuardError::Unsupported {
            construct: "join alias",
        });
    }
    let left = join
        .larg
        .as_mut()
        .ok_or(GuardError::Unsupported { construct: "join" })?;
    bind_from_item(left, ctx)?;
    let right = join
        .rarg
        .as_mut()
        .ok_or(GuardError::Unsupported { construct: "join" })?;
    bind_from_item(right, ctx)?;
    if let Some(quals) = join.quals.as_mut() {
        let kind = admit_expr(quals, ctx)?;
        predicate_fold(&kind, ctx.catalog)?;
    }
    Ok(())
}

fn bind_range(var: &mut pb::RangeVar, ctx: &mut Ctx<'_>) -> Result<(), GuardError> {
    if !var.catalogname.is_empty() {
        return Err(GuardError::Unsupported {
            construct: "catalog qualifier",
        });
    }
    if !var.inh {
        return Err(GuardError::Unsupported { construct: "ONLY" });
    }
    if !var.relpersistence.is_empty() && var.relpersistence != "p" {
        return Err(GuardError::Unsupported {
            construct: "relation persistence",
        });
    }
    let alias = range_alias(var)?;
    if var.schemaname.is_empty() {
        return Err(GuardError::Unqualified {
            suggestion: unique_suggestion(ctx.catalog, &var.relname),
        });
    }
    if !closed_ident(&var.schemaname) || !closed_logical_ident(&var.relname) {
        return Err(GuardError::Unsupported {
            construct: "identifier",
        });
    }
    if schema_banned(&var.schemaname, ctx.surface) {
        return Err(GuardError::SystemSchema);
    }
    let rel_index = match find_relation(ctx.catalog, &var.schemaname, &var.relname) {
        Some(index) => index,
        None if var.schemaname == "_oo" => return Err(GuardError::SystemSchema),
        None => return Err(GuardError::RelationNotFound),
    };
    let rel = &ctx.catalog.relations[rel_index];
    let conflict = alias.clone().unwrap_or_else(|| rel.name.clone());
    if ctx
        .ranges
        .iter()
        .any(|range| conflict_name(range, ctx.catalog) == conflict)
    {
        return Err(GuardError::Unsupported {
            construct: "duplicate range name",
        });
    }
    let exposed = alias.clone().unwrap_or_else(|| rel.physical_name.clone());
    let ordinal = ctx.next_ordinal;
    ctx.next_ordinal = ctx.next_ordinal.saturating_add(1);
    var.schemaname.clone_from(&rel.physical_schema);
    var.relname.clone_from(&rel.physical_name);
    ctx.ranges.push(RangeBind {
        rel_index,
        alias,
        exposed,
        ordinal,
    });
    Ok(())
}

fn range_alias(var: &pb::RangeVar) -> Result<Option<String>, GuardError> {
    let Some(alias) = &var.alias else {
        return Ok(None);
    };
    if !alias.colnames.is_empty() {
        return Err(GuardError::Unsupported {
            construct: "column alias list",
        });
    }
    if !closed_ident(&alias.aliasname) {
        return Err(GuardError::Unsupported {
            construct: "identifier",
        });
    }
    Ok(Some(alias.aliasname.clone()))
}

fn conflict_name(range: &RangeBind, catalog: &PolicyCatalog) -> String {
    range
        .alias
        .clone()
        .unwrap_or_else(|| catalog.relations[range.rel_index].name.clone())
}

fn rewrite_targets(targets: &mut Vec<pb::Node>, ctx: &mut Ctx<'_>) -> Result<(), GuardError> {
    if targets.is_empty() {
        return Err(GuardError::Unsupported {
            construct: "empty target list",
        });
    }
    let original = std::mem::take(targets);
    let mut rewritten = Vec::with_capacity(original.len());
    for target in original {
        rewritten.extend(rewrite_one_target(target, ctx)?);
    }
    *targets = rewritten;
    Ok(())
}

fn rewrite_one_target(
    mut target: pb::Node,
    ctx: &mut Ctx<'_>,
) -> Result<Vec<pb::Node>, GuardError> {
    let (alias, star_names) = inspect_target(&target, ctx.surface == Surface::Physical)?;
    if let Some(names) = star_names {
        return expand_star(&names, &alias, ctx);
    }
    let kind = {
        let val = target_val(&mut target)?;
        admit_expr(val, ctx)?
    };
    if let Some(pb::node::Node::ResTarget(res)) = target.node.as_mut()
        && res.name.is_empty()
        && let ExprKind::Column {
            rel_index,
            column_index,
            ..
        } = &kind
    {
        res.name
            .clone_from(&ctx.catalog.relations[*rel_index].columns[*column_index].name);
    }
    let output_name = match target.node.as_ref() {
        Some(pb::node::Node::ResTarget(res)) => res.name.clone(),
        _ => String::new(),
    };
    ctx.outputs
        .push(output_item(&kind, &output_name, ctx.catalog));
    Ok(vec![target])
}

fn inspect_target(target: &pb::Node, generated: bool) -> Result<(String, Option<Vec<String>>), GuardError> {
    let res = match target.node.as_ref() {
        Some(pb::node::Node::ResTarget(res)) => res.as_ref(),
        _ => {
            return Err(GuardError::Unsupported {
                construct: "target",
            });
        }
    };
    if !res.indirection.is_empty() {
        return Err(GuardError::Unsupported {
            construct: "target indirection",
        });
    }
    if !res.name.is_empty()
        && !(generated && closed_logical_ident(&res.name) || closed_ident(&res.name)) {
        return Err(GuardError::Unsupported {
            construct: "output alias",
        });
    }
    let val = res.val.as_deref().ok_or(GuardError::Unsupported {
        construct: "target",
    })?;
    let star = star_names(val)?;
    Ok((res.name.clone(), star))
}

fn target_val(target: &mut pb::Node) -> Result<&mut pb::Node, GuardError> {
    match target.node.as_mut() {
        Some(pb::node::Node::ResTarget(res)) => res.val.as_mut().map(|val| val.as_mut()),
        _ => None,
    }
    .ok_or(GuardError::Unsupported {
        construct: "target",
    })
}

fn star_names(node: &pb::Node) -> Result<Option<Vec<String>>, GuardError> {
    let Some(pb::node::Node::ColumnRef(col)) = node.node.as_ref() else {
        return Ok(None);
    };
    let mut names = Vec::new();
    let mut saw_star = false;
    for field in &col.fields {
        match field.node.as_ref() {
            Some(pb::node::Node::String(value)) if !saw_star => names.push(value.sval.clone()),
            Some(pb::node::Node::AStar(_)) if !saw_star => saw_star = true,
            _ => {
                return Err(GuardError::Unsupported {
                    construct: "column reference",
                });
            }
        }
    }
    if !saw_star {
        return Ok(None);
    }
    if names.len() > 2 {
        return Err(GuardError::Unsupported {
            construct: "column reference",
        });
    }
    Ok(Some(names))
}

fn expand_star(
    names: &[String],
    output_alias: &str,
    ctx: &mut Ctx<'_>,
) -> Result<Vec<pb::Node>, GuardError> {
    if !output_alias.is_empty() {
        return Err(GuardError::Unsupported {
            construct: "star alias",
        });
    }
    let slots = if names.is_empty() {
        if ctx.ranges.is_empty() {
            return Err(GuardError::Unsupported { construct: "star" });
        }
        (0..ctx.ranges.len()).collect::<Vec<_>>()
    } else {
        one_qualifier(names, ctx)?
    };
    let mut nodes = Vec::new();
    for slot in slots {
        nodes.extend(expand_slot(slot, names, ctx)?);
    }
    Ok(nodes)
}

fn expand_slot(
    slot: usize,
    names: &[String],
    ctx: &mut Ctx<'_>,
) -> Result<Vec<pb::Node>, GuardError> {
    let rel_index = ctx.ranges[slot].rel_index;
    let exposed = ctx.ranges[slot].exposed.clone();
    let schema = ctx.catalog.relations[rel_index].schema.clone();
    let relation = ctx.catalog.relations[rel_index].name.clone();
    let columns = ctx.catalog.relations[rel_index]
        .columns
        .iter()
        .map(|col| (col.name.clone(), col.physical_name.clone(), col.type_oid))
        .collect::<Vec<_>>();
    ctx.starred.push(rel_index);
    ctx.expansions.push(StarExpansion {
        qualifier: names.join("."),
        schema: schema.clone(),
        name: relation.clone(),
        columns: columns
            .iter()
            .map(|(logical, _, _)| logical.clone())
            .collect(),
    });
    let mut nodes = Vec::with_capacity(columns.len());
    for (logical, physical, type_oid) in columns {
        ctx.outputs.push(OutputItem::Column(BoundOutputColumn {
            relation_schema: schema.clone(),
            relation_name: relation.clone(),
            name: logical.clone(),
            physical_name: physical.clone(),
            output_name: logical.clone(),
            type_oid,
        }));
        nodes.push(res_target_node(&logical, column_node(&exposed, &physical)));
    }
    Ok(nodes)
}

fn one_qualifier(names: &[String], ctx: &Ctx<'_>) -> Result<Vec<usize>, GuardError> {
    let slots = qualifier_slots(names, ctx);
    match slots.len() {
        1 => Ok(slots),
        0 => Err(column_error(
            &(0..ctx.ranges.len()).collect::<Vec<_>>(),
            ctx,
            false,
        )),
        _ => Err(column_error(&slots, ctx, true)),
    }
}

fn admit_where(
    where_clause: Option<&mut Box<pb::Node>>,
    ctx: &mut Ctx<'_>,
) -> Result<WhereFold, GuardError> {
    let Some(expr) = where_clause else {
        return Ok(WhereFold::Absent);
    };
    let kind = admit_expr(expr, ctx)?;
    let fold = predicate_fold(&kind, ctx.catalog)?;
    Ok(match fold {
        Fold::True => WhereFold::TrueConst,
        Fold::False | Fold::Other => WhereFold::Real,
    })
}

fn admit_sort(items: &mut [pb::Node], ctx: &mut Ctx<'_>) -> Result<(), GuardError> {
    for item in items {
        admit_sort_item(item, ctx)?;
    }
    Ok(())
}

fn admit_sort_item(node: &mut pb::Node, ctx: &mut Ctx<'_>) -> Result<(), GuardError> {
    let sort = match node.node.as_mut() {
        Some(pb::node::Node::SortBy(sort)) => sort.as_mut(),
        _ => {
            return Err(GuardError::Unsupported {
                construct: "order key",
            });
        }
    };
    if sort.sortby_dir >= SORT_USING || !sort.use_op.is_empty() {
        return Err(GuardError::Unsupported {
            construct: "order operator",
        });
    }
    if sort.sortby_nulls > pb::SortByNulls::SortbyNullsLast as i32 {
        return Err(GuardError::Unsupported {
            construct: "order nulls",
        });
    }
    let inner = sort.node.as_mut().ok_or(GuardError::Unsupported {
        construct: "order key",
    })?;
    let kind = admit_expr(inner, ctx)?;
    if !matches!(kind, ExprKind::Column { .. }) {
        return Err(GuardError::Unsupported {
            construct: "order key",
        });
    }
    Ok(())
}

fn admit_expr(node: &mut pb::Node, ctx: &mut Ctx<'_>) -> Result<ExprKind, GuardError> {
    match node.node.as_mut() {
        Some(pb::node::Node::ColumnRef(_)) => admit_column(node, ctx),
        Some(pb::node::Node::AConst(_)) => admit_const(node),
        Some(pb::node::Node::FuncCall(_)) => admit_func(node, ctx),
        Some(pb::node::Node::AExpr(_)) => admit_op(node, ctx),
        Some(pb::node::Node::BoolExpr(_)) => admit_bool(node, ctx),
        Some(pb::node::Node::NullTest(_)) => admit_null(node, ctx),
        Some(pb::node::Node::TypeCast(_)) => admit_literal_cast(node),
        Some(pb::node::Node::ParamRef(_)) => Err(GuardError::Unsupported {
            construct: "parameter",
        }),
        Some(pb::node::Node::SubLink(_)) | Some(pb::node::Node::RangeSubselect(_)) => {
            Err(GuardError::Unsupported {
                construct: "subquery",
            })
        }
        Some(pb::node::Node::CaseExpr(_)) => Err(GuardError::Unsupported { construct: "CASE" }),
        Some(_) => Err(GuardError::Unsupported {
            construct: "expression",
        }),
        None => Err(GuardError::Unsupported {
            construct: "expression",
        }),
    }
}

fn admit_column(node: &mut pb::Node, ctx: &mut Ctx<'_>) -> Result<ExprKind, GuardError> {
    let names = match node.node.as_ref() {
        Some(pb::node::Node::ColumnRef(col)) => plain_names(col)?,
        _ => {
            return Err(GuardError::Unsupported {
                construct: "column reference",
            });
        }
    };
    let (slot, column_index) = resolve_column(&names, ctx)?;
    let rel_index = ctx.ranges[slot].rel_index;
    let type_oid = ctx.catalog.relations[rel_index].columns[column_index].type_oid;
    let qualifier = ctx.ranges[slot].exposed.clone();
    let physical = ctx.catalog.relations[rel_index].columns[column_index]
        .physical_name
        .clone();
    if let Some(pb::node::Node::ColumnRef(col)) = node.node.as_mut() {
        col.fields = vec![string_node(&qualifier), string_node(&physical)];
    }
    Ok(ExprKind::Column {
        rel_index,
        column_index,
        type_oid,
    })
}

fn plain_names(col: &pb::ColumnRef) -> Result<Vec<String>, GuardError> {
    let mut names = Vec::with_capacity(col.fields.len());
    for field in &col.fields {
        match field.node.as_ref() {
            Some(pb::node::Node::String(value)) => names.push(value.sval.clone()),
            _ => {
                return Err(GuardError::Unsupported {
                    construct: "column reference",
                });
            }
        }
    }
    if names.is_empty() || names.len() > 3 {
        return Err(GuardError::Unsupported {
            construct: "column reference",
        });
    }
    Ok(names)
}

fn resolve_column(names: &[String], ctx: &Ctx<'_>) -> Result<(usize, usize), GuardError> {
    match names {
        [column] => {
            let hits = ctx
                .ranges
                .iter()
                .enumerate()
                .filter_map(|(slot, range)| {
                    column_index(ctx.catalog, range.rel_index, column).map(|_| slot)
                })
                .collect::<Vec<_>>();
            match hits.as_slice() {
                [slot] => {
                    let index = column_index(ctx.catalog, ctx.ranges[*slot].rel_index, column)
                        .ok_or(GuardError::ColumnNotAllowed {
                            available: Vec::new(),
                            pending: 0,
                            ambiguous: false,
                        })?;
                    Ok((*slot, index))
                }
                [] => Err(column_error(
                    &(0..ctx.ranges.len()).collect::<Vec<_>>(),
                    ctx,
                    false,
                )),
                many => Err(column_error(many, ctx, true)),
            }
        }
        [qualifier, column] => qualified_column(std::slice::from_ref(qualifier), column, ctx),
        [schema, relation, column] => {
            qualified_column(&[schema.clone(), relation.clone()], column, ctx)
        }
        _ => Err(GuardError::Unsupported {
            construct: "column reference",
        }),
    }
}

fn qualified_column(
    qualifier: &[String],
    column: &str,
    ctx: &Ctx<'_>,
) -> Result<(usize, usize), GuardError> {
    let slots = qualifier_slots(qualifier, ctx);
    match slots.as_slice() {
        [] => Err(column_error(
            &(0..ctx.ranges.len()).collect::<Vec<_>>(),
            ctx,
            false,
        )),
        [slot] => match column_index(ctx.catalog, ctx.ranges[*slot].rel_index, column) {
            Some(index) => Ok((*slot, index)),
            None => Err(column_error(&[*slot], ctx, false)),
        },
        many => Err(column_error(many, ctx, true)),
    }
}

fn qualifier_slots(names: &[String], ctx: &Ctx<'_>) -> Vec<usize> {
    ctx.ranges
        .iter()
        .enumerate()
        .filter(|(_, range)| qualifier_matches(range, ctx.catalog, names))
        .map(|(slot, _)| slot)
        .collect()
}

fn qualifier_matches(range: &RangeBind, catalog: &PolicyCatalog, names: &[String]) -> bool {
    let rel = &catalog.relations[range.rel_index];
    match names {
        [name] => match &range.alias {
            Some(alias) => alias == name,
            None => rel.name == name.as_str(),
        },
        [schema, name] => {
            range.alias.is_none() && rel.schema == schema.as_str() && rel.name == name.as_str()
        }
        _ => false,
    }
}

fn column_index(catalog: &PolicyCatalog, rel_index: usize, name: &str) -> Option<usize> {
    catalog.relations[rel_index]
        .columns
        .iter()
        .position(|column| column.name == name)
}

fn column_error(slots: &[usize], ctx: &Ctx<'_>, ambiguous: bool) -> GuardError {
    let mut available = Vec::new();
    let mut pending = 0_u32;
    for slot in slots {
        let range = &ctx.ranges[*slot];
        let rel = &ctx.catalog.relations[range.rel_index];
        pending = pending.saturating_add(rel.pending_unclassified);
        for column in &rel.columns {
            let label = if let Some(alias) = &range.alias {
                format!("{alias}.{}", column.name)
            } else {
                format!("{}.{}.{}", rel.schema, rel.name, column.name)
            };
            available.push(label);
        }
    }
    GuardError::ColumnNotAllowed {
        available,
        pending,
        ambiguous,
    }
}

fn admit_const(node: &pb::Node) -> Result<ExprKind, GuardError> {
    let Some(pb::node::Node::AConst(value)) = node.node.as_ref() else {
        return Err(GuardError::Unsupported {
            construct: "literal",
        });
    };
    if value.isnull {
        return Ok(ExprKind::Literal {
            kind: LiteralKind::Null,
            fold: Fold::Other,
        });
    }
    match &value.val {
        Some(pb::a_const::Val::Ival(_)) => Ok(ExprKind::Literal {
            kind: LiteralKind::Integer,
            fold: Fold::Other,
        }),
        Some(pb::a_const::Val::Sval(_)) => Ok(ExprKind::Literal {
            kind: LiteralKind::Text,
            fold: Fold::Other,
        }),
        Some(pb::a_const::Val::Boolval(value)) => Ok(bool_literal(value.boolval)),
        Some(pb::a_const::Val::Fval(_)) => Err(GuardError::Unsupported {
            construct: "float literal",
        }),
        Some(pb::a_const::Val::Bsval(_)) => Err(GuardError::Unsupported {
            construct: "bit string",
        }),
        None => Err(GuardError::Unsupported {
            construct: "literal",
        }),
    }
}

fn bool_literal(value: bool) -> ExprKind {
    ExprKind::Literal {
        kind: LiteralKind::Boolean,
        fold: if value { Fold::True } else { Fold::False },
    }
}

fn admit_literal_cast(node: &pb::Node) -> Result<ExprKind, GuardError> {
    let Some(pb::node::Node::TypeCast(cast)) = node.node.as_ref() else {
        return Err(GuardError::Unsupported { construct: "cast" });
    };
    let type_name = cast
        .type_name
        .as_ref()
        .ok_or(GuardError::Unsupported { construct: "cast" })?;
    let arg = cast
        .arg
        .as_deref()
        .ok_or(GuardError::Unsupported { construct: "cast" })?;
    if plain_bool_type(type_name) {
        return match bool_string(arg) {
            Some(value) => Ok(bool_literal(value)),
            None => Err(GuardError::Unsupported { construct: "cast" }),
        };
    }
    if plain_date_type(type_name)
        && let Some(text) = const_string(arg)
        && civil_date(text)
    {
        return Ok(ExprKind::Literal {
            kind: LiteralKind::Date,
            fold: Fold::Other,
        });
    }
    Err(GuardError::Unsupported { construct: "cast" })
}

fn plain_date_type(type_name: &pb::TypeName) -> bool {
    if type_name.setof
        || type_name.pct_type
        || !type_name.typmods.is_empty()
        || !type_name.array_bounds.is_empty()
    {
        return false;
    }
    let Ok(names) = string_fields(&type_name.names) else {
        return false;
    };
    match names.as_slice() {
        [name] => name == "date",
        [schema, name] => schema == "pg_catalog" && name == "date",
        _ => false,
    }
}

fn const_string(node: &pb::Node) -> Option<&str> {
    let pb::node::Node::AConst(value) = node.node.as_ref()? else {
        return None;
    };
    if value.isnull {
        return None;
    }
    match &value.val {
        Some(pb::a_const::Val::Sval(text)) => Some(text.sval.as_str()),
        _ => None,
    }
}

/// Gregorian `YYYY-MM-DD` with year `0001..=9999`. No time, zone, or typmod.
fn civil_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes[..4]
        .iter()
        .chain(&bytes[5..7])
        .chain(&bytes[8..])
        .all(u8::is_ascii_digit)
    {
        return false;
    }
    let year: u16 = text[..4].parse().unwrap_or(0);
    let month: u8 = text[5..7].parse().unwrap_or(0);
    let day: u8 = text[8..].parse().unwrap_or(0);
    if year < 1 {
        return false;
    }
    let last = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=last).contains(&day)
}

fn plain_bool_type(type_name: &pb::TypeName) -> bool {
    if type_name.setof
        || type_name.pct_type
        || !type_name.typmods.is_empty()
        || !type_name.array_bounds.is_empty()
    {
        return false;
    }
    let Ok(names) = string_fields(&type_name.names) else {
        return false;
    };
    match names.as_slice() {
        [name] => name == "bool" || name == "boolean",
        [schema, name] => schema == "pg_catalog" && (name == "bool" || name == "boolean"),
        _ => false,
    }
}

fn bool_string(node: &pb::Node) -> Option<bool> {
    let pb::node::Node::AConst(value) = node.node.as_ref()? else {
        return None;
    };
    if value.isnull {
        return None;
    }
    match &value.val {
        Some(pb::a_const::Val::Boolval(boolean)) => Some(boolean.boolval),
        Some(pb::a_const::Val::Sval(text)) => match text.sval.as_str() {
            "t" | "true" | "1" | "yes" | "on" => Some(true),
            "f" | "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn admit_func(node: &mut pb::Node, ctx: &mut Ctx<'_>) -> Result<ExprKind, GuardError> {
    let (schema, name, aggregate_star) = {
        let func = match node.node.as_ref() {
            Some(pb::node::Node::FuncCall(func)) => func.as_ref(),
            _ => {
                return Err(GuardError::Unsupported {
                    construct: "function",
                });
            }
        };
        if func.over.is_some()
            || func.agg_filter.is_some()
            || !func.agg_order.is_empty()
            || func.agg_within_group
            || func.agg_distinct
            || func.func_variadic
        {
            return Err(GuardError::Unsupported {
                construct: "function clause",
            });
        }
        let (schema, name) = function_name(&func.funcname)?;
        if func.agg_star && !func.args.is_empty() {
            return Err(GuardError::Unsupported {
                construct: "aggregate star",
            });
        }
        (schema, name, func.agg_star)
    };
    let types = if aggregate_star {
        Vec::new()
    } else {
        let args = match node.node.as_mut() {
            Some(pb::node::Node::FuncCall(func)) => &mut func.args,
            _ => {
                return Err(GuardError::Unsupported {
                    construct: "function",
                });
            }
        };
        let mut types = Vec::with_capacity(args.len());
        for arg in args {
            let kind = admit_expr(arg, ctx)?;
            types.push(infer_type(&kind, ctx.catalog).ok_or(GuardError::FunctionNotAllowed)?);
        }
        types
    };
    let resolved = resolve_function(
        ctx.catalog,
        schema.as_deref(),
        &name,
        &types,
        aggregate_star,
    )?;
    let bound = bound_function(resolved);
    ctx.functions.push(bound.clone());
    Ok(ExprKind::Function(bound))
}

fn function_name(nodes: &[pb::Node]) -> Result<(Option<String>, String), GuardError> {
    let names = string_fields(nodes)?;
    match names.as_slice() {
        [name] if closed_ident(name) => Ok((None, name.clone())),
        [schema, name] if function_schema_ok(schema) && closed_ident(name) => {
            Ok((Some(schema.clone()), name.clone()))
        }
        _ => Err(GuardError::FunctionNotAllowed),
    }
}

fn resolve_function<'a>(
    catalog: &'a PolicyCatalog,
    schema: Option<&str>,
    name: &str,
    args: &[u32],
    aggregate_star: bool,
) -> Result<&'a ResolvedFunction, GuardError> {
    let mut found = None;
    for function in &catalog.functions {
        if function.aggregate_star != aggregate_star || function.name != name {
            continue;
        }
        if let Some(schema) = schema
            && function.schema != schema
        {
            continue;
        }
        if function.argument_type_oids != args {
            continue;
        }
        if found.is_some() {
            return Err(GuardError::FunctionNotAllowed);
        }
        found = Some(function);
    }
    found.ok_or(GuardError::FunctionNotAllowed)
}

fn bound_function(function: &ResolvedFunction) -> BoundFunction {
    BoundFunction {
        oid: function.oid,
        schema: function.schema.clone(),
        name: function.name.clone(),
        aggregate_star: function.aggregate_star,
    }
}

fn admit_op(node: &mut pb::Node, ctx: &mut Ctx<'_>) -> Result<ExprKind, GuardError> {
    let kind = match node.node.as_ref() {
        Some(pb::node::Node::AExpr(expr)) => expr.kind,
        _ => {
            return Err(GuardError::Unsupported {
                construct: "operator expression",
            });
        }
    };
    if kind == AEXPR_IN {
        return admit_in(node, ctx);
    }
    let (schema, name) = {
        let expr = match node.node.as_ref() {
            Some(pb::node::Node::AExpr(expr)) => expr.as_ref(),
            _ => {
                return Err(GuardError::Unsupported {
                    construct: "operator expression",
                });
            }
        };
        if expr.kind != AEXPR_OP {
            return Err(operator_form(expr.kind));
        }
        if expr.lexpr.is_none() || expr.rexpr.is_none() {
            return Err(GuardError::Unsupported {
                construct: "unary operator",
            });
        }
        let (schema, name) = operator_name(&expr.name)?;
        (schema, name)
    };
    let compared = {
        let expr = match node.node.as_mut() {
            Some(pb::node::Node::AExpr(expr)) => expr.as_mut(),
            _ => {
                return Err(GuardError::Unsupported {
                    construct: "operator expression",
                });
            }
        };
        let left = expr.lexpr.as_mut().ok_or(GuardError::Unsupported {
            construct: "operator expression",
        })?;
        let left_kind = admit_expr(left, ctx)?;
        let right = expr.rexpr.as_mut().ok_or(GuardError::Unsupported {
            construct: "operator expression",
        })?;
        let right_kind = admit_expr(right, ctx)?;
        (
            infer_type(&left_kind, ctx.catalog).ok_or(GuardError::FunctionNotAllowed)?,
            infer_type(&right_kind, ctx.catalog).ok_or(GuardError::FunctionNotAllowed)?,
            matches!(
                left_kind,
                ExprKind::Literal {
                    kind: LiteralKind::Integer,
                    ..
                }
            ),
            matches!(
                right_kind,
                ExprKind::Literal {
                    kind: LiteralKind::Integer,
                    ..
                }
            ),
        )
    };
    let (left_type, right_type, left_int_literal, right_int_literal) = compared;
    let resolved = resolve_comparison(
        ctx.catalog,
        schema.as_deref(),
        &name,
        left_type,
        right_type,
        left_int_literal,
        right_int_literal,
    )?;
    let bound = BoundOperator {
        oid: resolved.oid,
        procedure_oid: resolved.procedure_oid,
        schema: resolved.schema.clone(),
        name: resolved.name.clone(),
    };
    ctx.operators.push(bound);
    Ok(ExprKind::Operator)
}

fn admit_in(node: &mut pb::Node, ctx: &mut Ctx<'_>) -> Result<ExprKind, GuardError> {
    let token = in_token(match node.node.as_ref() {
        Some(pb::node::Node::AExpr(expr)) => expr.name.as_slice(),
        _ => return Err(in_predicate()),
    })?;
    let left_type = {
        let left = match node.node.as_mut() {
            Some(pb::node::Node::AExpr(expr)) => expr.lexpr.as_mut(),
            _ => None,
        };
        let left = left.ok_or_else(in_predicate)?;
        admit_in_column(left, ctx)?
    };
    let items = in_list(node)?;
    for item in &items {
        admit_in_literal(item, left_type, token, ctx)?;
    }
    Ok(ExprKind::Operator)
}

fn in_list(node: &pb::Node) -> Result<Vec<pb::Node>, GuardError> {
    let right = match node.node.as_ref() {
        Some(pb::node::Node::AExpr(expr)) => expr.rexpr.as_deref(),
        _ => None,
    };
    let right = right.ok_or_else(in_predicate)?;
    let items = match right.node.as_ref() {
        Some(pb::node::Node::List(list)) => &list.items,
        _ => return Err(in_predicate()),
    };
    if items.is_empty() || items.len() > IN_LIST_LIMIT {
        return Err(in_predicate());
    }
    Ok(items.clone())
}

fn in_token(names: &[pb::Node]) -> Result<&'static str, GuardError> {
    let fields = string_fields(names).map_err(|_| in_predicate())?;
    match fields.as_slice() {
        [name] if name == "=" => Ok("="),
        [name] if name == "<>" => Ok("<>"),
        _ => Err(in_predicate()),
    }
}

fn admit_in_column(node: &mut pb::Node, ctx: &mut Ctx<'_>) -> Result<u32, GuardError> {
    refuse_closed_shape(node)?;
    if !matches!(node.node, Some(pb::node::Node::ColumnRef(_))) {
        return Err(in_predicate());
    }
    match admit_column(node, ctx)? {
        ExprKind::Column { type_oid, .. } => Ok(type_oid),
        _ => Err(in_predicate()),
    }
}

fn admit_in_literal(
    node: &pb::Node,
    left_type: u32,
    token: &str,
    ctx: &mut Ctx<'_>,
) -> Result<(), GuardError> {
    refuse_closed_shape(node)?;
    if !matches!(node.node, Some(pb::node::Node::AConst(_))) {
        // A column in the list is not granted, including one the snapshot hides.
        return Err(in_predicate());
    }
    let ExprKind::Literal { kind, .. } = admit_const(node)? else {
        return Err(in_predicate());
    };
    // NULL stays in the AST. Dropping it would make `NOT IN` true for rows
    // PostgreSQL leaves unknown. The same-type operator only authorizes it.
    let right_type = match kind {
        LiteralKind::Null => left_type,
        other => literal_type_oid(other, ctx.catalog.literal_types)
            .ok_or(GuardError::FunctionNotAllowed)?,
    };
    let resolved = resolve_comparison(
        ctx.catalog,
        None,
        token,
        left_type,
        right_type,
        false,
        matches!(kind, LiteralKind::Integer),
    )?;
    ctx.operators.push(BoundOperator {
        oid: resolved.oid,
        procedure_oid: resolved.procedure_oid,
        schema: resolved.schema.clone(),
        name: resolved.name.clone(),
    });
    Ok(())
}

fn literal_type_oid(kind: LiteralKind, types: LiteralTypes) -> Option<u32> {
    match kind {
        LiteralKind::Integer => types.integer,
        LiteralKind::Text => types.text,
        LiteralKind::Boolean => types.boolean,
        LiteralKind::Date => Some(Type::DATE.oid()),
        LiteralKind::Null => None,
    }
}

fn refuse_closed_shape(node: &pb::Node) -> Result<(), GuardError> {
    match node.node.as_ref() {
        Some(pb::node::Node::FuncCall(_)) => Err(unsupported("function")),
        Some(pb::node::Node::TypeCast(_)) => Err(unsupported("cast")),
        Some(pb::node::Node::ParamRef(_)) => Err(unsupported("parameter")),
        Some(pb::node::Node::SubLink(_)) | Some(pb::node::Node::RangeSubselect(_)) => {
            Err(unsupported("subquery"))
        }
        Some(pb::node::Node::AArrayExpr(_) | pb::node::Node::ArrayExpr(_)) => Err(in_predicate()),
        Some(pb::node::Node::AExpr(expr))
            if expr.kind == AEXPR_OP_ANY || expr.kind == AEXPR_OP_ALL =>
        {
            Err(unsupported("ANY predicate"))
        }
        Some(pb::node::Node::AExpr(expr)) if expr.lexpr.is_none() => {
            Err(unsupported("unary operator"))
        }
        _ => Ok(()),
    }
}

fn unsupported(construct: &'static str) -> GuardError {
    GuardError::Unsupported { construct }
}

fn in_predicate() -> GuardError {
    unsupported("IN predicate")
}

fn operator_form(kind: i32) -> GuardError {
    let construct = match pb::AExprKind::try_from(kind).ok() {
        Some(
            pb::AExprKind::AexprLike | pb::AExprKind::AexprIlike | pb::AExprKind::AexprSimilar,
        ) => "LIKE predicate",
        Some(pb::AExprKind::AexprIn) => "IN predicate",
        Some(
            pb::AExprKind::AexprBetween
            | pb::AExprKind::AexprNotBetween
            | pb::AExprKind::AexprBetweenSym
            | pb::AExprKind::AexprNotBetweenSym,
        ) => "BETWEEN predicate",
        Some(pb::AExprKind::AexprOpAny | pb::AExprKind::AexprOpAll) => "ANY predicate",
        _ => "operator expression",
    };
    GuardError::Unsupported { construct }
}

fn operator_name(nodes: &[pb::Node]) -> Result<(Option<String>, String), GuardError> {
    let names = string_fields(nodes)?;
    match names.as_slice() {
        [name] if operator_token_ok(name) => Ok((None, name.clone())),
        [schema, name] if function_schema_ok(schema) && operator_token_ok(name) => {
            Ok((Some(schema.clone()), name.clone()))
        }
        _ => Err(GuardError::FunctionNotAllowed),
    }
}

fn resolve_comparison<'a>(
    catalog: &'a PolicyCatalog,
    schema: Option<&str>,
    name: &str,
    left: u32,
    right: u32,
    left_int_literal: bool,
    right_int_literal: bool,
) -> Result<&'a ResolvedOperator, GuardError> {
    if let Ok(found) = resolve_operator(catalog, schema, name, left, right) {
        return Ok(found);
    }
    // PostgreSQL compares varchar with a text literal through its implicit
    // varchar-to-text cast. Bind only equality to the already verified builtin
    // text/text operator; all other casts and functions remain refused.
    if name == "="
        && let Some(text) = catalog.literal_types.text
        && [left, right].iter().all(|oid| *oid == text || *oid == Type::VARCHAR.oid())
        && (left == Type::VARCHAR.oid() || right == Type::VARCHAR.oid())
    {
        return resolve_operator(catalog, schema, name, text, text);
    }
    let numeric = Type::NUMERIC.oid();
    let Some(integer) = catalog.literal_types.integer else {
        return Err(GuardError::FunctionNotAllowed);
    };
    let promotes = (left_int_literal && left == integer && right == numeric)
        || (right_int_literal && right == integer && left == numeric);
    if promotes {
        return resolve_operator(catalog, schema, name, numeric, numeric);
    }
    Err(GuardError::FunctionNotAllowed)
}

fn resolve_operator<'a>(
    catalog: &'a PolicyCatalog,
    schema: Option<&str>,
    name: &str,
    left: u32,
    right: u32,
) -> Result<&'a ResolvedOperator, GuardError> {
    let mut found = None;
    for operator in &catalog.operators {
        if operator.name != name
            || operator.left_type_oid != left
            || operator.right_type_oid != right
        {
            continue;
        }
        if let Some(schema) = schema
            && operator.schema != schema
        {
            continue;
        }
        if found.is_some() {
            return Err(GuardError::FunctionNotAllowed);
        }
        found = Some(operator);
    }
    found.ok_or(GuardError::FunctionNotAllowed)
}

fn admit_bool(node: &mut pb::Node, ctx: &mut Ctx<'_>) -> Result<ExprKind, GuardError> {
    let op = match node.node.as_ref() {
        Some(pb::node::Node::BoolExpr(expr)) => expr.boolop,
        _ => {
            return Err(GuardError::Unsupported {
                construct: "boolean",
            });
        }
    };
    let count = match node.node.as_ref() {
        Some(pb::node::Node::BoolExpr(expr)) => expr.args.len(),
        _ => 0,
    };
    match op {
        BOOL_AND | BOOL_OR if count >= 2 => {}
        BOOL_NOT if count == 1 => {}
        _ => {
            return Err(GuardError::Unsupported {
                construct: "boolean",
            });
        }
    }
    let mut folds = Vec::with_capacity(count);
    let args = match node.node.as_mut() {
        Some(pb::node::Node::BoolExpr(expr)) => &mut expr.args,
        _ => {
            return Err(GuardError::Unsupported {
                construct: "boolean",
            });
        }
    };
    for arg in args {
        let kind = admit_expr(arg, ctx)?;
        folds.push(predicate_fold(&kind, ctx.catalog)?);
    }
    let fold = match op {
        BOOL_AND => fold_and(&folds),
        BOOL_OR => fold_or(&folds),
        _ => match folds.first() {
            Some(Fold::True) => Fold::False,
            Some(Fold::False) => Fold::True,
            _ => Fold::Other,
        },
    };
    Ok(ExprKind::Bool(fold))
}

fn fold_and(parts: &[Fold]) -> Fold {
    if parts.contains(&Fold::False) {
        Fold::False
    } else if parts.iter().all(|fold| *fold == Fold::True) {
        Fold::True
    } else {
        Fold::Other
    }
}

fn fold_or(parts: &[Fold]) -> Fold {
    if parts.contains(&Fold::True) {
        Fold::True
    } else if parts.iter().all(|fold| *fold == Fold::False) {
        Fold::False
    } else {
        Fold::Other
    }
}

fn admit_null(node: &mut pb::Node, ctx: &mut Ctx<'_>) -> Result<ExprKind, GuardError> {
    if !ctx.catalog.null_test_allowed {
        return Err(GuardError::FunctionNotAllowed);
    }
    let (test, row) = match node.node.as_ref() {
        Some(pb::node::Node::NullTest(test)) => (test.nulltesttype, test.argisrow),
        _ => {
            return Err(GuardError::Unsupported {
                construct: "null test",
            });
        }
    };
    if row || !matches!(test, NULL_IS | NULL_NOT) {
        return Err(GuardError::Unsupported {
            construct: "null test",
        });
    }
    let arg = match node.node.as_mut() {
        Some(pb::node::Node::NullTest(test)) => test.arg.as_mut(),
        _ => None,
    };
    let arg = arg.ok_or(GuardError::Unsupported {
        construct: "null test",
    })?;
    let kind = admit_expr(arg, ctx)?;
    if !matches!(kind, ExprKind::Column { .. }) {
        return Err(GuardError::Unsupported {
            construct: "null test",
        });
    }
    ctx.null_tests = ctx.null_tests.saturating_add(1);
    Ok(ExprKind::NullTest)
}

fn predicate_fold(kind: &ExprKind, catalog: &PolicyCatalog) -> Result<Fold, GuardError> {
    match kind {
        ExprKind::Literal {
            kind: LiteralKind::Boolean,
            fold,
        } => Ok(*fold),
        ExprKind::Bool(fold) => Ok(*fold),
        ExprKind::Operator | ExprKind::NullTest => Ok(Fold::Other),
        ExprKind::Column { type_oid, .. } if catalog.literal_types.boolean == Some(*type_oid) => {
            Ok(Fold::Other)
        }
        _ => Err(GuardError::Unsupported {
            construct: "predicate",
        }),
    }
}

fn infer_type(kind: &ExprKind, catalog: &PolicyCatalog) -> Option<u32> {
    match kind {
        ExprKind::Column { type_oid, .. } => Some(*type_oid),
        ExprKind::Literal {
            kind: LiteralKind::Integer,
            ..
        } => catalog.literal_types.integer,
        ExprKind::Literal {
            kind: LiteralKind::Text,
            ..
        } => catalog.literal_types.text,
        ExprKind::Literal {
            kind: LiteralKind::Boolean,
            ..
        }
        | ExprKind::Bool(_)
        | ExprKind::Operator
        | ExprKind::NullTest => catalog.literal_types.boolean,
        ExprKind::Literal {
            kind: LiteralKind::Date,
            ..
        } => Some(Type::DATE.oid()),
        ExprKind::Literal {
            kind: LiteralKind::Null,
            ..
        }
        | ExprKind::Function(_) => None,
    }
}

fn output_item(kind: &ExprKind, output_name: &str, catalog: &PolicyCatalog) -> OutputItem {
    match kind {
        ExprKind::Column {
            rel_index,
            column_index,
            type_oid,
        } => {
            let relation = &catalog.relations[*rel_index];
            let column = &relation.columns[*column_index];
            let name = if output_name.is_empty() {
                column.name.clone()
            } else {
                output_name.to_string()
            };
            OutputItem::Column(BoundOutputColumn {
                relation_schema: relation.schema.clone(),
                relation_name: relation.name.clone(),
                name: column.name.clone(),
                physical_name: column.physical_name.clone(),
                output_name: name,
                type_oid: *type_oid,
            })
        }
        ExprKind::Function(function) => OutputItem::Function(function.clone()),
        ExprKind::Literal { kind, .. } => OutputItem::Literal(*kind),
        ExprKind::Operator | ExprKind::Bool(_) | ExprKind::NullTest => OutputItem::Compound,
    }
}

fn large_relation(admitted: &Admitted, catalog: &PolicyCatalog) -> Option<String> {
    if !matches!(
        admitted.where_fold,
        WhereFold::Absent | WhereFold::TrueConst
    ) {
        return None;
    }
    admitted.starred.iter().find_map(|index| {
        let relation = &catalog.relations[*index];
        if relation.estimated_rows > catalog.large_table_row_threshold {
            Some(format!("{}.{}", relation.schema, relation.name))
        } else {
            None
        }
    })
}

fn ordered_relation(range: &RangeBind, catalog: &PolicyCatalog) -> OrderedRelation {
    let relation = &catalog.relations[range.rel_index];
    OrderedRelation {
        ordinal: range.ordinal,
        bound: BoundRelation {
            schema: relation.schema.clone(),
            name: relation.name.clone(),
            alias: range.alias.clone(),
            physical_schema: relation.physical_schema.clone(),
            physical_name: relation.physical_name.clone(),
        },
    }
}

fn string_node(value: &str) -> pb::Node {
    pb::Node {
        node: Some(pb::node::Node::String(pb::String {
            sval: value.to_string(),
        })),
    }
}

fn column_node(qualifier: &str, column: &str) -> pb::Node {
    pb::Node {
        node: Some(pb::node::Node::ColumnRef(pb::ColumnRef {
            fields: vec![string_node(qualifier), string_node(column)],
            location: -1,
        })),
    }
}

fn res_target_node(name: &str, value: pb::Node) -> pb::Node {
    pb::Node {
        node: Some(pb::node::Node::ResTarget(Box::new(pb::ResTarget {
            name: name.to_string(),
            indirection: Vec::new(),
            val: Some(Box::new(value)),
            location: -1,
        }))),
    }
}

fn string_fields(nodes: &[pb::Node]) -> Result<Vec<String>, GuardError> {
    let mut names = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node.node.as_ref() {
            Some(pb::node::Node::String(value)) => names.push(value.sval.clone()),
            _ => {
                return Err(GuardError::Unsupported {
                    construct: "identifier",
                });
            }
        }
    }
    Ok(names)
}

fn physical_catalog(catalog: &PolicyCatalog) -> PolicyCatalog {
    let relations = catalog
        .relations
        .iter()
        .map(|relation| VisibleRelation {
            schema: relation.physical_schema.clone(),
            name: relation.physical_name.clone(),
            physical_schema: relation.physical_schema.clone(),
            physical_name: relation.physical_name.clone(),
            columns: relation
                .columns
                .iter()
                .map(|column| VisibleColumn {
                    name: column.physical_name.clone(),
                    physical_name: column.physical_name.clone(),
                    type_oid: column.type_oid,
                })
                .collect(),
            pending_unclassified: relation.pending_unclassified,
            estimated_rows: relation.estimated_rows,
        })
        .collect();
    PolicyCatalog {
        identity: catalog.identity.clone(),
        relations,
        functions: catalog.functions.clone(),
        operators: catalog.operators.clone(),
        literal_types: catalog.literal_types,
        null_test_allowed: catalog.null_test_allowed,
        large_table_row_threshold: catalog.large_table_row_threshold,
    }
}

fn validate(
    identity: &PolicyIdentity,
    catalog: &PolicyCatalog,
    surface: Surface,
) -> Result<(), GuardError> {
    validate_identity_text(identity)?;
    validate_identity_text(&catalog.identity)?;
    if identity != &catalog.identity {
        return Err(GuardError::IdentityMismatch);
    }
    validate_relations(catalog, surface)?;
    validate_functions(catalog)?;
    validate_operators(catalog)?;
    for oid in [
        catalog.literal_types.integer,
        catalog.literal_types.text,
        catalog.literal_types.boolean,
    ]
    .into_iter()
    .flatten()
    {
        if oid == 0 {
            return Err(GuardError::CatalogInvalid { reason: "type oid" });
        }
    }
    Ok(())
}

fn validate_identity_text(identity: &PolicyIdentity) -> Result<(), GuardError> {
    if identity_text(&identity.project)
        && identity_text(&identity.role)
        && identity_text(&identity.policy_id)
    {
        Ok(())
    } else {
        Err(GuardError::CatalogInvalid { reason: "identity" })
    }
}

fn identity_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.chars().any(|char| char == '\0' || char.is_control())
}

fn validate_relations(catalog: &PolicyCatalog, surface: Surface) -> Result<(), GuardError> {
    let mut logical = BTreeSet::new();
    let mut physical = BTreeSet::new();
    for relation in &catalog.relations {
        validate_relation(relation, surface)?;
        if !logical.insert((relation.schema.clone(), relation.name.clone())) {
            return Err(GuardError::CatalogInvalid {
                reason: "duplicate relation",
            });
        }
        if !physical.insert((
            relation.physical_schema.clone(),
            relation.physical_name.clone(),
        )) {
            return Err(GuardError::CatalogInvalid {
                reason: "duplicate relation",
            });
        }
    }
    Ok(())
}

fn validate_relation(relation: &VisibleRelation, surface: Surface) -> Result<(), GuardError> {
    if !closed_ident(&relation.schema) || !closed_logical_ident(&relation.name) {
        return Err(GuardError::CatalogInvalid {
            reason: "identifier",
        });
    }
    if schema_banned(&relation.schema, surface) {
        return Err(GuardError::CatalogInvalid {
            reason: "relation schema",
        });
    }
    if !closed_ident(&relation.physical_schema) || !closed_ident(&relation.physical_name) {
        return Err(GuardError::CatalogInvalid {
            reason: "identifier",
        });
    }
    if physical_schema_banned(&relation.physical_schema) {
        return Err(GuardError::CatalogInvalid {
            reason: "physical schema",
        });
    }
    if relation.columns.is_empty() {
        return Err(GuardError::CatalogInvalid {
            reason: "empty column list",
        });
    }
    let mut names = BTreeSet::new();
    let mut physical = BTreeSet::new();
    for column in &relation.columns {
        if !closed_logical_ident(&column.name) || !closed_ident(&column.physical_name) {
            return Err(GuardError::CatalogInvalid {
                reason: "identifier",
            });
        }
        if column.type_oid == 0 {
            return Err(GuardError::CatalogInvalid { reason: "type oid" });
        }
        if !names.insert(column.name.clone()) || !physical.insert(column.physical_name.clone()) {
            return Err(GuardError::CatalogInvalid {
                reason: "duplicate column",
            });
        }
    }
    Ok(())
}

fn validate_functions(catalog: &PolicyCatalog) -> Result<(), GuardError> {
    let mut seen = BTreeSet::new();
    for function in &catalog.functions {
        if function.oid == 0 || function.argument_type_oids.contains(&0) {
            return Err(GuardError::CatalogInvalid {
                reason: "function oid",
            });
        }
        if !function_schema_ok(&function.schema) || !closed_ident(&function.name) {
            return Err(GuardError::CatalogInvalid {
                reason: "function schema",
            });
        }
        if function.aggregate_star && !function.argument_type_oids.is_empty() {
            return Err(GuardError::CatalogInvalid {
                reason: "function oid",
            });
        }
        let signature = (
            function.schema.clone(),
            function.name.clone(),
            function.argument_type_oids.clone(),
            function.aggregate_star,
        );
        if !seen.insert(signature) {
            return Err(GuardError::CatalogInvalid {
                reason: "duplicate function",
            });
        }
    }
    Ok(())
}

fn validate_operators(catalog: &PolicyCatalog) -> Result<(), GuardError> {
    let mut seen = BTreeSet::new();
    for operator in &catalog.operators {
        if operator.oid == 0
            || operator.procedure_oid == 0
            || operator.left_type_oid == 0
            || operator.right_type_oid == 0
        {
            return Err(GuardError::CatalogInvalid {
                reason: "operator oid",
            });
        }
        if !function_schema_ok(&operator.schema) || !operator_token_ok(&operator.name) {
            return Err(GuardError::CatalogInvalid {
                reason: "operator name",
            });
        }
        let signature = (
            operator.schema.clone(),
            operator.name.clone(),
            operator.left_type_oid,
            operator.right_type_oid,
        );
        if !seen.insert(signature) {
            return Err(GuardError::CatalogInvalid {
                reason: "duplicate operator",
            });
        }
    }
    Ok(())
}

fn schema_banned(schema: &str, surface: Surface) -> bool {
    if physical_schema_banned(schema) {
        return true;
    }
    surface == Surface::User && schema == "_oo"
}

fn physical_schema_banned(schema: &str) -> bool {
    schema == "pg_catalog" || schema == "information_schema" || schema.ends_with("__stage")
}

// Resolved function and operator schemas may be `pg_catalog`.
// Relation lookup still rejects that schema through `schema_banned`.
fn function_schema_ok(schema: &str) -> bool {
    closed_ident(schema)
        && schema != "information_schema"
        && schema != "_oo"
        && !schema.ends_with("__stage")
}

fn closed_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first = bytes[0];
    if first != b'_' && !first.is_ascii_lowercase() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn closed_logical_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && (bytes[0] == b'_' || bytes[0].is_ascii_alphabetic())
        && bytes[1..]
            .iter()
            .all(|byte| *byte == b'_' || byte.is_ascii_alphanumeric())
}

fn operator_token_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes.iter().all(|byte| {
            byte.is_ascii()
                && !byte.is_ascii_control()
                && !byte.is_ascii_whitespace()
                && !matches!(byte, b'\'' | b'"' | b'\\' | b';' | b'\0')
        })
}

fn unique_suggestion(catalog: &PolicyCatalog, relname: &str) -> Option<String> {
    let mut found = None;
    for relation in &catalog.relations {
        if relation.name != relname {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(format!("{}.{}", relation.schema, relation.name));
    }
    found
}

fn find_relation(catalog: &PolicyCatalog, schema: &str, name: &str) -> Option<usize> {
    catalog
        .relations
        .iter()
        .position(|relation| relation.schema == schema && relation.name == name)
}

fn column_message(available: &[String], pending: u32, ambiguous: bool) -> String {
    let mut message = if ambiguous {
        "column reference is ambiguous".to_string()
    } else {
        "column is not visible".to_string()
    };
    if !available.is_empty() {
        message.push_str(": ");
        message.push_str(&available.join(", "));
    }
    if pending > 0 {
        message.push_str(&format!("; {pending} columns are pending classification"));
    }
    message
}

fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
}

fn descend_node(node: &pb::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    scan.enter(depth)?;
    match node.node.as_ref() {
        Some(kind) => descend_kind(kind, depth, scan),
        None => Ok(()),
    }
}

// The functions below walk every protobuf node variant shipped in pg_query 6.2.1.
// They count nodes, enforce depth, and record Insert, Update, Delete, and Merge.
// They are not a permission check.

fn descend_kind(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::Alias(_)
        | pb::node::Node::RangeVar(_)
        | pb::node::Node::TableFunc(_)
        | pb::node::Node::IntoClause(_)
        | pb::node::Node::Var(_)
        | pb::node::Node::Param(_)
        | pb::node::Node::Aggref(_)
        | pb::node::Node::GroupingFunc(_)
        | pb::node::Node::WindowFunc(_)
        | pb::node::Node::WindowFuncRunCondition(_)
        | pb::node::Node::MergeSupportFunc(_)
        | pb::node::Node::SubscriptingRef(_)
        | pb::node::Node::FuncExpr(_)
        | pb::node::Node::NamedArgExpr(_)
        | pb::node::Node::OpExpr(_)
        | pb::node::Node::DistinctExpr(_) => descend_kind_0(kind, depth, scan),
        pb::node::Node::NullIfExpr(_)
        | pb::node::Node::ScalarArrayOpExpr(_)
        | pb::node::Node::BoolExpr(_)
        | pb::node::Node::SubLink(_)
        | pb::node::Node::SubPlan(_)
        | pb::node::Node::AlternativeSubPlan(_)
        | pb::node::Node::FieldSelect(_)
        | pb::node::Node::FieldStore(_)
        | pb::node::Node::RelabelType(_)
        | pb::node::Node::CoerceViaIo(_)
        | pb::node::Node::ArrayCoerceExpr(_)
        | pb::node::Node::ConvertRowtypeExpr(_)
        | pb::node::Node::CollateExpr(_)
        | pb::node::Node::CaseExpr(_)
        | pb::node::Node::CaseWhen(_)
        | pb::node::Node::CaseTestExpr(_) => descend_kind_1(kind, depth, scan),
        pb::node::Node::ArrayExpr(_)
        | pb::node::Node::RowExpr(_)
        | pb::node::Node::RowCompareExpr(_)
        | pb::node::Node::CoalesceExpr(_)
        | pb::node::Node::MinMaxExpr(_)
        | pb::node::Node::SqlvalueFunction(_)
        | pb::node::Node::XmlExpr(_)
        | pb::node::Node::JsonFormat(_)
        | pb::node::Node::JsonReturning(_)
        | pb::node::Node::JsonValueExpr(_)
        | pb::node::Node::JsonConstructorExpr(_)
        | pb::node::Node::JsonIsPredicate(_)
        | pb::node::Node::JsonBehavior(_)
        | pb::node::Node::JsonExpr(_)
        | pb::node::Node::JsonTablePath(_)
        | pb::node::Node::JsonTablePathScan(_) => descend_kind_2(kind, depth, scan),
        pb::node::Node::JsonTableSiblingJoin(_)
        | pb::node::Node::NullTest(_)
        | pb::node::Node::BooleanTest(_)
        | pb::node::Node::MergeAction(_)
        | pb::node::Node::CoerceToDomain(_)
        | pb::node::Node::CoerceToDomainValue(_)
        | pb::node::Node::SetToDefault(_)
        | pb::node::Node::CurrentOfExpr(_)
        | pb::node::Node::NextValueExpr(_)
        | pb::node::Node::InferenceElem(_)
        | pb::node::Node::TargetEntry(_)
        | pb::node::Node::RangeTblRef(_)
        | pb::node::Node::JoinExpr(_)
        | pb::node::Node::FromExpr(_)
        | pb::node::Node::OnConflictExpr(_)
        | pb::node::Node::Query(_) => descend_kind_3(kind, depth, scan),
        pb::node::Node::TypeName(_)
        | pb::node::Node::ColumnRef(_)
        | pb::node::Node::ParamRef(_)
        | pb::node::Node::AExpr(_)
        | pb::node::Node::TypeCast(_)
        | pb::node::Node::CollateClause(_)
        | pb::node::Node::RoleSpec(_)
        | pb::node::Node::FuncCall(_)
        | pb::node::Node::AStar(_)
        | pb::node::Node::AIndices(_)
        | pb::node::Node::AIndirection(_)
        | pb::node::Node::AArrayExpr(_)
        | pb::node::Node::ResTarget(_)
        | pb::node::Node::MultiAssignRef(_)
        | pb::node::Node::SortBy(_)
        | pb::node::Node::WindowDef(_) => descend_kind_4(kind, depth, scan),
        pb::node::Node::RangeSubselect(_)
        | pb::node::Node::RangeFunction(_)
        | pb::node::Node::RangeTableFunc(_)
        | pb::node::Node::RangeTableFuncCol(_)
        | pb::node::Node::RangeTableSample(_)
        | pb::node::Node::ColumnDef(_)
        | pb::node::Node::TableLikeClause(_)
        | pb::node::Node::IndexElem(_)
        | pb::node::Node::DefElem(_)
        | pb::node::Node::LockingClause(_)
        | pb::node::Node::XmlSerialize(_)
        | pb::node::Node::PartitionElem(_)
        | pb::node::Node::PartitionSpec(_)
        | pb::node::Node::PartitionBoundSpec(_)
        | pb::node::Node::PartitionRangeDatum(_)
        | pb::node::Node::SinglePartitionSpec(_) => descend_kind_5(kind, depth, scan),
        pb::node::Node::PartitionCmd(_)
        | pb::node::Node::RangeTblEntry(_)
        | pb::node::Node::RtepermissionInfo(_)
        | pb::node::Node::RangeTblFunction(_)
        | pb::node::Node::TableSampleClause(_)
        | pb::node::Node::WithCheckOption(_)
        | pb::node::Node::SortGroupClause(_)
        | pb::node::Node::GroupingSet(_)
        | pb::node::Node::WindowClause(_)
        | pb::node::Node::RowMarkClause(_)
        | pb::node::Node::WithClause(_)
        | pb::node::Node::InferClause(_)
        | pb::node::Node::OnConflictClause(_)
        | pb::node::Node::CtesearchClause(_)
        | pb::node::Node::CtecycleClause(_)
        | pb::node::Node::CommonTableExpr(_) => descend_kind_6(kind, depth, scan),
        pb::node::Node::MergeWhenClause(_)
        | pb::node::Node::TriggerTransition(_)
        | pb::node::Node::JsonOutput(_)
        | pb::node::Node::JsonArgument(_)
        | pb::node::Node::JsonFuncExpr(_)
        | pb::node::Node::JsonTablePathSpec(_)
        | pb::node::Node::JsonTable(_)
        | pb::node::Node::JsonTableColumn(_)
        | pb::node::Node::JsonKeyValue(_)
        | pb::node::Node::JsonParseExpr(_)
        | pb::node::Node::JsonScalarExpr(_)
        | pb::node::Node::JsonSerializeExpr(_)
        | pb::node::Node::JsonObjectConstructor(_)
        | pb::node::Node::JsonArrayConstructor(_)
        | pb::node::Node::JsonArrayQueryConstructor(_)
        | pb::node::Node::JsonAggConstructor(_) => descend_kind_7(kind, depth, scan),
        pb::node::Node::JsonObjectAgg(_)
        | pb::node::Node::JsonArrayAgg(_)
        | pb::node::Node::RawStmt(_)
        | pb::node::Node::InsertStmt(_)
        | pb::node::Node::DeleteStmt(_)
        | pb::node::Node::UpdateStmt(_)
        | pb::node::Node::MergeStmt(_)
        | pb::node::Node::SelectStmt(_)
        | pb::node::Node::SetOperationStmt(_)
        | pb::node::Node::ReturnStmt(_)
        | pb::node::Node::PlassignStmt(_)
        | pb::node::Node::CreateSchemaStmt(_)
        | pb::node::Node::AlterTableStmt(_)
        | pb::node::Node::ReplicaIdentityStmt(_)
        | pb::node::Node::AlterTableCmd(_)
        | pb::node::Node::AlterCollationStmt(_) => descend_kind_8(kind, depth, scan),
        pb::node::Node::AlterDomainStmt(_)
        | pb::node::Node::GrantStmt(_)
        | pb::node::Node::ObjectWithArgs(_)
        | pb::node::Node::AccessPriv(_)
        | pb::node::Node::GrantRoleStmt(_)
        | pb::node::Node::AlterDefaultPrivilegesStmt(_)
        | pb::node::Node::CopyStmt(_)
        | pb::node::Node::VariableSetStmt(_)
        | pb::node::Node::VariableShowStmt(_)
        | pb::node::Node::CreateStmt(_)
        | pb::node::Node::Constraint(_)
        | pb::node::Node::CreateTableSpaceStmt(_)
        | pb::node::Node::DropTableSpaceStmt(_)
        | pb::node::Node::AlterTableSpaceOptionsStmt(_)
        | pb::node::Node::AlterTableMoveAllStmt(_)
        | pb::node::Node::CreateExtensionStmt(_) => descend_kind_9(kind, depth, scan),
        pb::node::Node::AlterExtensionStmt(_)
        | pb::node::Node::AlterExtensionContentsStmt(_)
        | pb::node::Node::CreateFdwStmt(_)
        | pb::node::Node::AlterFdwStmt(_)
        | pb::node::Node::CreateForeignServerStmt(_)
        | pb::node::Node::AlterForeignServerStmt(_)
        | pb::node::Node::CreateForeignTableStmt(_)
        | pb::node::Node::CreateUserMappingStmt(_)
        | pb::node::Node::AlterUserMappingStmt(_)
        | pb::node::Node::DropUserMappingStmt(_)
        | pb::node::Node::ImportForeignSchemaStmt(_)
        | pb::node::Node::CreatePolicyStmt(_)
        | pb::node::Node::AlterPolicyStmt(_)
        | pb::node::Node::CreateAmStmt(_)
        | pb::node::Node::CreateTrigStmt(_)
        | pb::node::Node::CreateEventTrigStmt(_) => descend_kind_10(kind, depth, scan),
        pb::node::Node::AlterEventTrigStmt(_)
        | pb::node::Node::CreatePlangStmt(_)
        | pb::node::Node::CreateRoleStmt(_)
        | pb::node::Node::AlterRoleStmt(_)
        | pb::node::Node::AlterRoleSetStmt(_)
        | pb::node::Node::DropRoleStmt(_)
        | pb::node::Node::CreateSeqStmt(_)
        | pb::node::Node::AlterSeqStmt(_)
        | pb::node::Node::DefineStmt(_)
        | pb::node::Node::CreateDomainStmt(_)
        | pb::node::Node::CreateOpClassStmt(_)
        | pb::node::Node::CreateOpClassItem(_)
        | pb::node::Node::CreateOpFamilyStmt(_)
        | pb::node::Node::AlterOpFamilyStmt(_)
        | pb::node::Node::DropStmt(_)
        | pb::node::Node::TruncateStmt(_) => descend_kind_11(kind, depth, scan),
        pb::node::Node::CommentStmt(_)
        | pb::node::Node::SecLabelStmt(_)
        | pb::node::Node::DeclareCursorStmt(_)
        | pb::node::Node::ClosePortalStmt(_)
        | pb::node::Node::FetchStmt(_)
        | pb::node::Node::IndexStmt(_)
        | pb::node::Node::CreateStatsStmt(_)
        | pb::node::Node::StatsElem(_)
        | pb::node::Node::AlterStatsStmt(_)
        | pb::node::Node::CreateFunctionStmt(_)
        | pb::node::Node::FunctionParameter(_)
        | pb::node::Node::AlterFunctionStmt(_)
        | pb::node::Node::DoStmt(_)
        | pb::node::Node::InlineCodeBlock(_)
        | pb::node::Node::CallStmt(_)
        | pb::node::Node::CallContext(_) => descend_kind_12(kind, depth, scan),
        pb::node::Node::RenameStmt(_)
        | pb::node::Node::AlterObjectDependsStmt(_)
        | pb::node::Node::AlterObjectSchemaStmt(_)
        | pb::node::Node::AlterOwnerStmt(_)
        | pb::node::Node::AlterOperatorStmt(_)
        | pb::node::Node::AlterTypeStmt(_)
        | pb::node::Node::RuleStmt(_)
        | pb::node::Node::NotifyStmt(_)
        | pb::node::Node::ListenStmt(_)
        | pb::node::Node::UnlistenStmt(_)
        | pb::node::Node::TransactionStmt(_)
        | pb::node::Node::CompositeTypeStmt(_)
        | pb::node::Node::CreateEnumStmt(_)
        | pb::node::Node::CreateRangeStmt(_)
        | pb::node::Node::AlterEnumStmt(_)
        | pb::node::Node::ViewStmt(_) => descend_kind_13(kind, depth, scan),
        pb::node::Node::LoadStmt(_)
        | pb::node::Node::CreatedbStmt(_)
        | pb::node::Node::AlterDatabaseStmt(_)
        | pb::node::Node::AlterDatabaseRefreshCollStmt(_)
        | pb::node::Node::AlterDatabaseSetStmt(_)
        | pb::node::Node::DropdbStmt(_)
        | pb::node::Node::AlterSystemStmt(_)
        | pb::node::Node::ClusterStmt(_)
        | pb::node::Node::VacuumStmt(_)
        | pb::node::Node::VacuumRelation(_)
        | pb::node::Node::ExplainStmt(_)
        | pb::node::Node::CreateTableAsStmt(_)
        | pb::node::Node::RefreshMatViewStmt(_)
        | pb::node::Node::CheckPointStmt(_)
        | pb::node::Node::DiscardStmt(_)
        | pb::node::Node::LockStmt(_) => descend_kind_14(kind, depth, scan),
        pb::node::Node::ConstraintsSetStmt(_)
        | pb::node::Node::ReindexStmt(_)
        | pb::node::Node::CreateConversionStmt(_)
        | pb::node::Node::CreateCastStmt(_)
        | pb::node::Node::CreateTransformStmt(_)
        | pb::node::Node::PrepareStmt(_)
        | pb::node::Node::ExecuteStmt(_)
        | pb::node::Node::DeallocateStmt(_)
        | pb::node::Node::DropOwnedStmt(_)
        | pb::node::Node::ReassignOwnedStmt(_)
        | pb::node::Node::AlterTsdictionaryStmt(_)
        | pb::node::Node::AlterTsconfigurationStmt(_)
        | pb::node::Node::PublicationTable(_)
        | pb::node::Node::PublicationObjSpec(_)
        | pb::node::Node::CreatePublicationStmt(_)
        | pb::node::Node::AlterPublicationStmt(_) => descend_kind_15(kind, depth, scan),
        pb::node::Node::CreateSubscriptionStmt(_)
        | pb::node::Node::AlterSubscriptionStmt(_)
        | pb::node::Node::DropSubscriptionStmt(_)
        | pb::node::Node::Integer(_)
        | pb::node::Node::Float(_)
        | pb::node::Node::Boolean(_)
        | pb::node::Node::String(_)
        | pb::node::Node::BitString(_)
        | pb::node::Node::List(_)
        | pb::node::Node::IntList(_)
        | pb::node::Node::OidList(_)
        | pb::node::Node::AConst(_) => descend_kind_16(kind, depth, scan),
    }
}

fn descend_kind_0(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::Alias(inner) => descend_alias(inner, depth, scan),
        pb::node::Node::RangeVar(inner) => descend_range_var(inner, depth, scan),
        pb::node::Node::TableFunc(inner) => descend_table_func(inner, depth, scan),
        pb::node::Node::IntoClause(inner) => descend_into_clause(inner, depth, scan),
        pb::node::Node::Var(inner) => descend_var(inner, depth, scan),
        pb::node::Node::Param(inner) => descend_param(inner, depth, scan),
        pb::node::Node::Aggref(inner) => descend_aggref(inner, depth, scan),
        pb::node::Node::GroupingFunc(inner) => descend_grouping_func(inner, depth, scan),
        pb::node::Node::WindowFunc(inner) => descend_window_func(inner, depth, scan),
        pb::node::Node::WindowFuncRunCondition(inner) => {
            descend_window_func_run_condition(inner, depth, scan)
        }
        pb::node::Node::MergeSupportFunc(inner) => descend_merge_support_func(inner, depth, scan),
        pb::node::Node::SubscriptingRef(inner) => descend_subscripting_ref(inner, depth, scan),
        pb::node::Node::FuncExpr(inner) => descend_func_expr(inner, depth, scan),
        pb::node::Node::NamedArgExpr(inner) => descend_named_arg_expr(inner, depth, scan),
        pb::node::Node::OpExpr(inner) => descend_op_expr(inner, depth, scan),
        pb::node::Node::DistinctExpr(inner) => descend_distinct_expr(inner, depth, scan),
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_1(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::NullIfExpr(inner) => descend_null_if_expr(inner, depth, scan),
        pb::node::Node::ScalarArrayOpExpr(inner) => {
            descend_scalar_array_op_expr(inner, depth, scan)
        }
        pb::node::Node::BoolExpr(inner) => descend_bool_expr(inner, depth, scan),
        pb::node::Node::SubLink(inner) => descend_sub_link(inner, depth, scan),
        pb::node::Node::SubPlan(inner) => descend_sub_plan(inner, depth, scan),
        pb::node::Node::AlternativeSubPlan(inner) => {
            descend_alternative_sub_plan(inner, depth, scan)
        }
        pb::node::Node::FieldSelect(inner) => descend_field_select(inner, depth, scan),
        pb::node::Node::FieldStore(inner) => descend_field_store(inner, depth, scan),
        pb::node::Node::RelabelType(inner) => descend_relabel_type(inner, depth, scan),
        pb::node::Node::CoerceViaIo(inner) => descend_coerce_via_io(inner, depth, scan),
        pb::node::Node::ArrayCoerceExpr(inner) => descend_array_coerce_expr(inner, depth, scan),
        pb::node::Node::ConvertRowtypeExpr(inner) => {
            descend_convert_rowtype_expr(inner, depth, scan)
        }
        pb::node::Node::CollateExpr(inner) => descend_collate_expr(inner, depth, scan),
        pb::node::Node::CaseExpr(inner) => descend_case_expr(inner, depth, scan),
        pb::node::Node::CaseWhen(inner) => descend_case_when(inner, depth, scan),
        pb::node::Node::CaseTestExpr(inner) => descend_case_test_expr(inner, depth, scan),
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_2(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::ArrayExpr(inner) => descend_array_expr(inner, depth, scan),
        pb::node::Node::RowExpr(inner) => descend_row_expr(inner, depth, scan),
        pb::node::Node::RowCompareExpr(inner) => descend_row_compare_expr(inner, depth, scan),
        pb::node::Node::CoalesceExpr(inner) => descend_coalesce_expr(inner, depth, scan),
        pb::node::Node::MinMaxExpr(inner) => descend_min_max_expr(inner, depth, scan),
        pb::node::Node::SqlvalueFunction(inner) => descend_sql_value_function(inner, depth, scan),
        pb::node::Node::XmlExpr(inner) => descend_xml_expr(inner, depth, scan),
        pb::node::Node::JsonFormat(inner) => descend_json_format(inner, depth, scan),
        pb::node::Node::JsonReturning(inner) => descend_json_returning(inner, depth, scan),
        pb::node::Node::JsonValueExpr(inner) => descend_json_value_expr(inner, depth, scan),
        pb::node::Node::JsonConstructorExpr(inner) => {
            descend_json_constructor_expr(inner, depth, scan)
        }
        pb::node::Node::JsonIsPredicate(inner) => descend_json_is_predicate(inner, depth, scan),
        pb::node::Node::JsonBehavior(inner) => descend_json_behavior(inner, depth, scan),
        pb::node::Node::JsonExpr(inner) => descend_json_expr(inner, depth, scan),
        pb::node::Node::JsonTablePath(inner) => descend_json_table_path(inner, depth, scan),
        pb::node::Node::JsonTablePathScan(inner) => {
            descend_json_table_path_scan(inner, depth, scan)
        }
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_3(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::JsonTableSiblingJoin(inner) => {
            descend_json_table_sibling_join(inner, depth, scan)
        }
        pb::node::Node::NullTest(inner) => descend_null_test(inner, depth, scan),
        pb::node::Node::BooleanTest(inner) => descend_boolean_test(inner, depth, scan),
        pb::node::Node::MergeAction(inner) => descend_merge_action(inner, depth, scan),
        pb::node::Node::CoerceToDomain(inner) => descend_coerce_to_domain(inner, depth, scan),
        pb::node::Node::CoerceToDomainValue(inner) => {
            descend_coerce_to_domain_value(inner, depth, scan)
        }
        pb::node::Node::SetToDefault(inner) => descend_set_to_default(inner, depth, scan),
        pb::node::Node::CurrentOfExpr(inner) => descend_current_of_expr(inner, depth, scan),
        pb::node::Node::NextValueExpr(inner) => descend_next_value_expr(inner, depth, scan),
        pb::node::Node::InferenceElem(inner) => descend_inference_elem(inner, depth, scan),
        pb::node::Node::TargetEntry(inner) => descend_target_entry(inner, depth, scan),
        pb::node::Node::RangeTblRef(inner) => descend_range_tbl_ref(inner, depth, scan),
        pb::node::Node::JoinExpr(inner) => descend_join_expr(inner, depth, scan),
        pb::node::Node::FromExpr(inner) => descend_from_expr(inner, depth, scan),
        pb::node::Node::OnConflictExpr(inner) => descend_on_conflict_expr(inner, depth, scan),
        pb::node::Node::Query(inner) => descend_query(inner, depth, scan),
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_4(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::TypeName(inner) => descend_type_name(inner, depth, scan),
        pb::node::Node::ColumnRef(inner) => descend_column_ref(inner, depth, scan),
        pb::node::Node::ParamRef(inner) => descend_param_ref(inner, depth, scan),
        pb::node::Node::AExpr(inner) => descend_a_expr(inner, depth, scan),
        pb::node::Node::TypeCast(inner) => descend_type_cast(inner, depth, scan),
        pb::node::Node::CollateClause(inner) => descend_collate_clause(inner, depth, scan),
        pb::node::Node::RoleSpec(inner) => descend_role_spec(inner, depth, scan),
        pb::node::Node::FuncCall(inner) => descend_func_call(inner, depth, scan),
        pb::node::Node::AStar(inner) => descend_a_star(inner, depth, scan),
        pb::node::Node::AIndices(inner) => descend_a_indices(inner, depth, scan),
        pb::node::Node::AIndirection(inner) => descend_a_indirection(inner, depth, scan),
        pb::node::Node::AArrayExpr(inner) => descend_a_array_expr(inner, depth, scan),
        pb::node::Node::ResTarget(inner) => descend_res_target(inner, depth, scan),
        pb::node::Node::MultiAssignRef(inner) => descend_multi_assign_ref(inner, depth, scan),
        pb::node::Node::SortBy(inner) => descend_sort_by(inner, depth, scan),
        pb::node::Node::WindowDef(inner) => descend_window_def(inner, depth, scan),
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_5(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::RangeSubselect(inner) => descend_range_subselect(inner, depth, scan),
        pb::node::Node::RangeFunction(inner) => descend_range_function(inner, depth, scan),
        pb::node::Node::RangeTableFunc(inner) => descend_range_table_func(inner, depth, scan),
        pb::node::Node::RangeTableFuncCol(inner) => {
            descend_range_table_func_col(inner, depth, scan)
        }
        pb::node::Node::RangeTableSample(inner) => descend_range_table_sample(inner, depth, scan),
        pb::node::Node::ColumnDef(inner) => descend_column_def(inner, depth, scan),
        pb::node::Node::TableLikeClause(inner) => descend_table_like_clause(inner, depth, scan),
        pb::node::Node::IndexElem(inner) => descend_index_elem(inner, depth, scan),
        pb::node::Node::DefElem(inner) => descend_def_elem(inner, depth, scan),
        pb::node::Node::LockingClause(inner) => descend_locking_clause(inner, depth, scan),
        pb::node::Node::XmlSerialize(inner) => descend_xml_serialize(inner, depth, scan),
        pb::node::Node::PartitionElem(inner) => descend_partition_elem(inner, depth, scan),
        pb::node::Node::PartitionSpec(inner) => descend_partition_spec(inner, depth, scan),
        pb::node::Node::PartitionBoundSpec(inner) => {
            descend_partition_bound_spec(inner, depth, scan)
        }
        pb::node::Node::PartitionRangeDatum(inner) => {
            descend_partition_range_datum(inner, depth, scan)
        }
        pb::node::Node::SinglePartitionSpec(inner) => {
            descend_single_partition_spec(inner, depth, scan)
        }
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_6(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::PartitionCmd(inner) => descend_partition_cmd(inner, depth, scan),
        pb::node::Node::RangeTblEntry(inner) => descend_range_tbl_entry(inner, depth, scan),
        pb::node::Node::RtepermissionInfo(inner) => descend_rte_permission_info(inner, depth, scan),
        pb::node::Node::RangeTblFunction(inner) => descend_range_tbl_function(inner, depth, scan),
        pb::node::Node::TableSampleClause(inner) => descend_table_sample_clause(inner, depth, scan),
        pb::node::Node::WithCheckOption(inner) => descend_with_check_option(inner, depth, scan),
        pb::node::Node::SortGroupClause(inner) => descend_sort_group_clause(inner, depth, scan),
        pb::node::Node::GroupingSet(inner) => descend_grouping_set(inner, depth, scan),
        pb::node::Node::WindowClause(inner) => descend_window_clause(inner, depth, scan),
        pb::node::Node::RowMarkClause(inner) => descend_row_mark_clause(inner, depth, scan),
        pb::node::Node::WithClause(inner) => descend_with_clause(inner, depth, scan),
        pb::node::Node::InferClause(inner) => descend_infer_clause(inner, depth, scan),
        pb::node::Node::OnConflictClause(inner) => descend_on_conflict_clause(inner, depth, scan),
        pb::node::Node::CtesearchClause(inner) => descend_cte_search_clause(inner, depth, scan),
        pb::node::Node::CtecycleClause(inner) => descend_cte_cycle_clause(inner, depth, scan),
        pb::node::Node::CommonTableExpr(inner) => descend_common_table_expr(inner, depth, scan),
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_7(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::MergeWhenClause(inner) => descend_merge_when_clause(inner, depth, scan),
        pb::node::Node::TriggerTransition(inner) => descend_trigger_transition(inner, depth, scan),
        pb::node::Node::JsonOutput(inner) => descend_json_output(inner, depth, scan),
        pb::node::Node::JsonArgument(inner) => descend_json_argument(inner, depth, scan),
        pb::node::Node::JsonFuncExpr(inner) => descend_json_func_expr(inner, depth, scan),
        pb::node::Node::JsonTablePathSpec(inner) => {
            descend_json_table_path_spec(inner, depth, scan)
        }
        pb::node::Node::JsonTable(inner) => descend_json_table(inner, depth, scan),
        pb::node::Node::JsonTableColumn(inner) => descend_json_table_column(inner, depth, scan),
        pb::node::Node::JsonKeyValue(inner) => descend_json_key_value(inner, depth, scan),
        pb::node::Node::JsonParseExpr(inner) => descend_json_parse_expr(inner, depth, scan),
        pb::node::Node::JsonScalarExpr(inner) => descend_json_scalar_expr(inner, depth, scan),
        pb::node::Node::JsonSerializeExpr(inner) => descend_json_serialize_expr(inner, depth, scan),
        pb::node::Node::JsonObjectConstructor(inner) => {
            descend_json_object_constructor(inner, depth, scan)
        }
        pb::node::Node::JsonArrayConstructor(inner) => {
            descend_json_array_constructor(inner, depth, scan)
        }
        pb::node::Node::JsonArrayQueryConstructor(inner) => {
            descend_json_array_query_constructor(inner, depth, scan)
        }
        pb::node::Node::JsonAggConstructor(inner) => {
            descend_json_agg_constructor(inner, depth, scan)
        }
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_8(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::JsonObjectAgg(inner) => descend_json_object_agg(inner, depth, scan),
        pb::node::Node::JsonArrayAgg(inner) => descend_json_array_agg(inner, depth, scan),
        pb::node::Node::RawStmt(inner) => descend_raw_stmt(inner, depth, scan),
        pb::node::Node::InsertStmt(inner) => {
            scan.saw_write = true;
            descend_insert_stmt(inner, depth, scan)
        }
        pb::node::Node::DeleteStmt(inner) => {
            scan.saw_write = true;
            descend_delete_stmt(inner, depth, scan)
        }
        pb::node::Node::UpdateStmt(inner) => {
            scan.saw_write = true;
            descend_update_stmt(inner, depth, scan)
        }
        pb::node::Node::MergeStmt(inner) => {
            scan.saw_write = true;
            descend_merge_stmt(inner, depth, scan)
        }
        pb::node::Node::SelectStmt(inner) => descend_select_stmt(inner, depth, scan),
        pb::node::Node::SetOperationStmt(inner) => descend_set_operation_stmt(inner, depth, scan),
        pb::node::Node::ReturnStmt(inner) => descend_return_stmt(inner, depth, scan),
        pb::node::Node::PlassignStmt(inner) => descend_pl_assign_stmt(inner, depth, scan),
        pb::node::Node::CreateSchemaStmt(inner) => descend_create_schema_stmt(inner, depth, scan),
        pb::node::Node::AlterTableStmt(inner) => descend_alter_table_stmt(inner, depth, scan),
        pb::node::Node::ReplicaIdentityStmt(inner) => {
            descend_replica_identity_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterTableCmd(inner) => descend_alter_table_cmd(inner, depth, scan),
        pb::node::Node::AlterCollationStmt(inner) => {
            descend_alter_collation_stmt(inner, depth, scan)
        }
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_9(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::AlterDomainStmt(inner) => descend_alter_domain_stmt(inner, depth, scan),
        pb::node::Node::GrantStmt(inner) => descend_grant_stmt(inner, depth, scan),
        pb::node::Node::ObjectWithArgs(inner) => descend_object_with_args(inner, depth, scan),
        pb::node::Node::AccessPriv(inner) => descend_access_priv(inner, depth, scan),
        pb::node::Node::GrantRoleStmt(inner) => descend_grant_role_stmt(inner, depth, scan),
        pb::node::Node::AlterDefaultPrivilegesStmt(inner) => {
            descend_alter_default_privileges_stmt(inner, depth, scan)
        }
        pb::node::Node::CopyStmt(inner) => descend_copy_stmt(inner, depth, scan),
        pb::node::Node::VariableSetStmt(inner) => descend_variable_set_stmt(inner, depth, scan),
        pb::node::Node::VariableShowStmt(inner) => descend_variable_show_stmt(inner, depth, scan),
        pb::node::Node::CreateStmt(inner) => descend_create_stmt(inner, depth, scan),
        pb::node::Node::Constraint(inner) => descend_constraint(inner, depth, scan),
        pb::node::Node::CreateTableSpaceStmt(inner) => {
            descend_create_table_space_stmt(inner, depth, scan)
        }
        pb::node::Node::DropTableSpaceStmt(inner) => {
            descend_drop_table_space_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterTableSpaceOptionsStmt(inner) => {
            descend_alter_table_space_options_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterTableMoveAllStmt(inner) => {
            descend_alter_table_move_all_stmt(inner, depth, scan)
        }
        pb::node::Node::CreateExtensionStmt(inner) => {
            descend_create_extension_stmt(inner, depth, scan)
        }
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_10(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::AlterExtensionStmt(inner) => {
            descend_alter_extension_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterExtensionContentsStmt(inner) => {
            descend_alter_extension_contents_stmt(inner, depth, scan)
        }
        pb::node::Node::CreateFdwStmt(inner) => descend_create_fdw_stmt(inner, depth, scan),
        pb::node::Node::AlterFdwStmt(inner) => descend_alter_fdw_stmt(inner, depth, scan),
        pb::node::Node::CreateForeignServerStmt(inner) => {
            descend_create_foreign_server_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterForeignServerStmt(inner) => {
            descend_alter_foreign_server_stmt(inner, depth, scan)
        }
        pb::node::Node::CreateForeignTableStmt(inner) => {
            descend_create_foreign_table_stmt(inner, depth, scan)
        }
        pb::node::Node::CreateUserMappingStmt(inner) => {
            descend_create_user_mapping_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterUserMappingStmt(inner) => {
            descend_alter_user_mapping_stmt(inner, depth, scan)
        }
        pb::node::Node::DropUserMappingStmt(inner) => {
            descend_drop_user_mapping_stmt(inner, depth, scan)
        }
        pb::node::Node::ImportForeignSchemaStmt(inner) => {
            descend_import_foreign_schema_stmt(inner, depth, scan)
        }
        pb::node::Node::CreatePolicyStmt(inner) => descend_create_policy_stmt(inner, depth, scan),
        pb::node::Node::AlterPolicyStmt(inner) => descend_alter_policy_stmt(inner, depth, scan),
        pb::node::Node::CreateAmStmt(inner) => descend_create_am_stmt(inner, depth, scan),
        pb::node::Node::CreateTrigStmt(inner) => descend_create_trig_stmt(inner, depth, scan),
        pb::node::Node::CreateEventTrigStmt(inner) => {
            descend_create_event_trig_stmt(inner, depth, scan)
        }
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_11(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::AlterEventTrigStmt(inner) => {
            descend_alter_event_trig_stmt(inner, depth, scan)
        }
        pb::node::Node::CreatePlangStmt(inner) => descend_create_p_lang_stmt(inner, depth, scan),
        pb::node::Node::CreateRoleStmt(inner) => descend_create_role_stmt(inner, depth, scan),
        pb::node::Node::AlterRoleStmt(inner) => descend_alter_role_stmt(inner, depth, scan),
        pb::node::Node::AlterRoleSetStmt(inner) => descend_alter_role_set_stmt(inner, depth, scan),
        pb::node::Node::DropRoleStmt(inner) => descend_drop_role_stmt(inner, depth, scan),
        pb::node::Node::CreateSeqStmt(inner) => descend_create_seq_stmt(inner, depth, scan),
        pb::node::Node::AlterSeqStmt(inner) => descend_alter_seq_stmt(inner, depth, scan),
        pb::node::Node::DefineStmt(inner) => descend_define_stmt(inner, depth, scan),
        pb::node::Node::CreateDomainStmt(inner) => descend_create_domain_stmt(inner, depth, scan),
        pb::node::Node::CreateOpClassStmt(inner) => {
            descend_create_op_class_stmt(inner, depth, scan)
        }
        pb::node::Node::CreateOpClassItem(inner) => {
            descend_create_op_class_item(inner, depth, scan)
        }
        pb::node::Node::CreateOpFamilyStmt(inner) => {
            descend_create_op_family_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterOpFamilyStmt(inner) => {
            descend_alter_op_family_stmt(inner, depth, scan)
        }
        pb::node::Node::DropStmt(inner) => descend_drop_stmt(inner, depth, scan),
        pb::node::Node::TruncateStmt(inner) => descend_truncate_stmt(inner, depth, scan),
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_12(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::CommentStmt(inner) => descend_comment_stmt(inner, depth, scan),
        pb::node::Node::SecLabelStmt(inner) => descend_sec_label_stmt(inner, depth, scan),
        pb::node::Node::DeclareCursorStmt(inner) => descend_declare_cursor_stmt(inner, depth, scan),
        pb::node::Node::ClosePortalStmt(inner) => descend_close_portal_stmt(inner, depth, scan),
        pb::node::Node::FetchStmt(inner) => descend_fetch_stmt(inner, depth, scan),
        pb::node::Node::IndexStmt(inner) => descend_index_stmt(inner, depth, scan),
        pb::node::Node::CreateStatsStmt(inner) => descend_create_stats_stmt(inner, depth, scan),
        pb::node::Node::StatsElem(inner) => descend_stats_elem(inner, depth, scan),
        pb::node::Node::AlterStatsStmt(inner) => descend_alter_stats_stmt(inner, depth, scan),
        pb::node::Node::CreateFunctionStmt(inner) => {
            descend_create_function_stmt(inner, depth, scan)
        }
        pb::node::Node::FunctionParameter(inner) => descend_function_parameter(inner, depth, scan),
        pb::node::Node::AlterFunctionStmt(inner) => descend_alter_function_stmt(inner, depth, scan),
        pb::node::Node::DoStmt(inner) => descend_do_stmt(inner, depth, scan),
        pb::node::Node::InlineCodeBlock(inner) => descend_inline_code_block(inner, depth, scan),
        pb::node::Node::CallStmt(inner) => descend_call_stmt(inner, depth, scan),
        pb::node::Node::CallContext(inner) => descend_call_context(inner, depth, scan),
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_13(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::RenameStmt(inner) => descend_rename_stmt(inner, depth, scan),
        pb::node::Node::AlterObjectDependsStmt(inner) => {
            descend_alter_object_depends_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterObjectSchemaStmt(inner) => {
            descend_alter_object_schema_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterOwnerStmt(inner) => descend_alter_owner_stmt(inner, depth, scan),
        pb::node::Node::AlterOperatorStmt(inner) => descend_alter_operator_stmt(inner, depth, scan),
        pb::node::Node::AlterTypeStmt(inner) => descend_alter_type_stmt(inner, depth, scan),
        pb::node::Node::RuleStmt(inner) => descend_rule_stmt(inner, depth, scan),
        pb::node::Node::NotifyStmt(inner) => descend_notify_stmt(inner, depth, scan),
        pb::node::Node::ListenStmt(inner) => descend_listen_stmt(inner, depth, scan),
        pb::node::Node::UnlistenStmt(inner) => descend_unlisten_stmt(inner, depth, scan),
        pb::node::Node::TransactionStmt(inner) => descend_transaction_stmt(inner, depth, scan),
        pb::node::Node::CompositeTypeStmt(inner) => descend_composite_type_stmt(inner, depth, scan),
        pb::node::Node::CreateEnumStmt(inner) => descend_create_enum_stmt(inner, depth, scan),
        pb::node::Node::CreateRangeStmt(inner) => descend_create_range_stmt(inner, depth, scan),
        pb::node::Node::AlterEnumStmt(inner) => descend_alter_enum_stmt(inner, depth, scan),
        pb::node::Node::ViewStmt(inner) => descend_view_stmt(inner, depth, scan),
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_14(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::LoadStmt(inner) => descend_load_stmt(inner, depth, scan),
        pb::node::Node::CreatedbStmt(inner) => descend_createdb_stmt(inner, depth, scan),
        pb::node::Node::AlterDatabaseStmt(inner) => descend_alter_database_stmt(inner, depth, scan),
        pb::node::Node::AlterDatabaseRefreshCollStmt(inner) => {
            descend_alter_database_refresh_coll_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterDatabaseSetStmt(inner) => {
            descend_alter_database_set_stmt(inner, depth, scan)
        }
        pb::node::Node::DropdbStmt(inner) => descend_dropdb_stmt(inner, depth, scan),
        pb::node::Node::AlterSystemStmt(inner) => descend_alter_system_stmt(inner, depth, scan),
        pb::node::Node::ClusterStmt(inner) => descend_cluster_stmt(inner, depth, scan),
        pb::node::Node::VacuumStmt(inner) => descend_vacuum_stmt(inner, depth, scan),
        pb::node::Node::VacuumRelation(inner) => descend_vacuum_relation(inner, depth, scan),
        pb::node::Node::ExplainStmt(inner) => descend_explain_stmt(inner, depth, scan),
        pb::node::Node::CreateTableAsStmt(inner) => {
            descend_create_table_as_stmt(inner, depth, scan)
        }
        pb::node::Node::RefreshMatViewStmt(inner) => {
            descend_refresh_mat_view_stmt(inner, depth, scan)
        }
        pb::node::Node::CheckPointStmt(inner) => descend_check_point_stmt(inner, depth, scan),
        pb::node::Node::DiscardStmt(inner) => descend_discard_stmt(inner, depth, scan),
        pb::node::Node::LockStmt(inner) => descend_lock_stmt(inner, depth, scan),
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_15(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::ConstraintsSetStmt(inner) => {
            descend_constraints_set_stmt(inner, depth, scan)
        }
        pb::node::Node::ReindexStmt(inner) => descend_reindex_stmt(inner, depth, scan),
        pb::node::Node::CreateConversionStmt(inner) => {
            descend_create_conversion_stmt(inner, depth, scan)
        }
        pb::node::Node::CreateCastStmt(inner) => descend_create_cast_stmt(inner, depth, scan),
        pb::node::Node::CreateTransformStmt(inner) => {
            descend_create_transform_stmt(inner, depth, scan)
        }
        pb::node::Node::PrepareStmt(inner) => descend_prepare_stmt(inner, depth, scan),
        pb::node::Node::ExecuteStmt(inner) => descend_execute_stmt(inner, depth, scan),
        pb::node::Node::DeallocateStmt(inner) => descend_deallocate_stmt(inner, depth, scan),
        pb::node::Node::DropOwnedStmt(inner) => descend_drop_owned_stmt(inner, depth, scan),
        pb::node::Node::ReassignOwnedStmt(inner) => descend_reassign_owned_stmt(inner, depth, scan),
        pb::node::Node::AlterTsdictionaryStmt(inner) => {
            descend_alter_ts_dictionary_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterTsconfigurationStmt(inner) => {
            descend_alter_ts_configuration_stmt(inner, depth, scan)
        }
        pb::node::Node::PublicationTable(inner) => descend_publication_table(inner, depth, scan),
        pb::node::Node::PublicationObjSpec(inner) => {
            descend_publication_obj_spec(inner, depth, scan)
        }
        pb::node::Node::CreatePublicationStmt(inner) => {
            descend_create_publication_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterPublicationStmt(inner) => {
            descend_alter_publication_stmt(inner, depth, scan)
        }
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_kind_16(kind: &pb::node::Node, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    match kind {
        pb::node::Node::CreateSubscriptionStmt(inner) => {
            descend_create_subscription_stmt(inner, depth, scan)
        }
        pb::node::Node::AlterSubscriptionStmt(inner) => {
            descend_alter_subscription_stmt(inner, depth, scan)
        }
        pb::node::Node::DropSubscriptionStmt(inner) => {
            descend_drop_subscription_stmt(inner, depth, scan)
        }
        pb::node::Node::Integer(inner) => descend_integer(inner, depth, scan),
        pb::node::Node::Float(inner) => descend_float(inner, depth, scan),
        pb::node::Node::Boolean(inner) => descend_boolean(inner, depth, scan),
        pb::node::Node::String(inner) => descend_string(inner, depth, scan),
        pb::node::Node::BitString(inner) => descend_bit_string(inner, depth, scan),
        pb::node::Node::List(inner) => descend_list(inner, depth, scan),
        pb::node::Node::IntList(inner) => descend_int_list(inner, depth, scan),
        pb::node::Node::OidList(inner) => descend_oid_list(inner, depth, scan),
        pb::node::Node::AConst(inner) => descend_a_const(inner, depth, scan),
        _ => Err(GuardError::Unsupported {
            construct: "ast node",
        }),
    }
}

fn descend_a_array_expr(
    value: &pb::AArrayExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.elements {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_a_const(_value: &pb::AConst, _depth: usize, _scan: &mut Scan) -> Result<(), GuardError> {
    Ok(())
}

fn descend_a_expr(value: &pb::AExpr, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    for node in &value.name {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.lexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.rexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_a_indices(
    value: &pb::AIndices,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.lidx {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.uidx {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_a_indirection(
    value: &pb::AIndirection,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.indirection {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_a_star(_value: &pb::AStar, _depth: usize, _scan: &mut Scan) -> Result<(), GuardError> {
    Ok(())
}

fn descend_access_priv(
    value: &pb::AccessPriv,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.cols {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_aggref(value: &pb::Aggref, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.aggargtypes {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.aggdirectargs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.aggorder {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.aggdistinct {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.aggfilter {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alias(value: &pb::Alias, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    for node in &value.colnames {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_collation_stmt(
    value: &pb::AlterCollationStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.collname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_database_refresh_coll_stmt(
    _value: &pb::AlterDatabaseRefreshCollStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_alter_database_set_stmt(
    value: &pb::AlterDatabaseSetStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.setstmt {
        descend_variable_set_stmt(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_database_stmt(
    value: &pb::AlterDatabaseStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_default_privileges_stmt(
    value: &pb::AlterDefaultPrivilegesStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.action {
        descend_grant_stmt(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_domain_stmt(
    value: &pb::AlterDomainStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.type_name {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.def {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_enum_stmt(
    value: &pb::AlterEnumStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.type_name {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_event_trig_stmt(
    _value: &pb::AlterEventTrigStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_alter_extension_contents_stmt(
    value: &pb::AlterExtensionContentsStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.object {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_extension_stmt(
    value: &pb::AlterExtensionStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_fdw_stmt(
    value: &pb::AlterFdwStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.func_options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_foreign_server_stmt(
    value: &pb::AlterForeignServerStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_function_stmt(
    value: &pb::AlterFunctionStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.func {
        descend_object_with_args(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.actions {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_object_depends_stmt(
    value: &pb::AlterObjectDependsStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.object {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.extname {
        descend_string(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_object_schema_stmt(
    value: &pb::AlterObjectSchemaStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.object {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_op_family_stmt(
    value: &pb::AlterOpFamilyStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.opfamilyname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.items {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_operator_stmt(
    value: &pb::AlterOperatorStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.opername {
        descend_object_with_args(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_owner_stmt(
    value: &pb::AlterOwnerStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.object {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.newowner {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_policy_stmt(
    value: &pb::AlterPolicyStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.table {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.roles {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.qual {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.with_check {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_publication_stmt(
    value: &pb::AlterPublicationStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.pubobjects {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_role_set_stmt(
    value: &pb::AlterRoleSetStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.role {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.setstmt {
        descend_variable_set_stmt(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_role_stmt(
    value: &pb::AlterRoleStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.role {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_seq_stmt(
    value: &pb::AlterSeqStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.sequence {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_stats_stmt(
    value: &pb::AlterStatsStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.defnames {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.stxstattarget {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_subscription_stmt(
    value: &pb::AlterSubscriptionStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.publication {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_system_stmt(
    value: &pb::AlterSystemStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.setstmt {
        descend_variable_set_stmt(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_table_cmd(
    value: &pb::AlterTableCmd,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.newowner {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.def {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_table_move_all_stmt(
    value: &pb::AlterTableMoveAllStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.roles {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_table_space_options_stmt(
    value: &pb::AlterTableSpaceOptionsStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_table_stmt(
    value: &pb::AlterTableStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.cmds {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_ts_configuration_stmt(
    value: &pb::AlterTsConfigurationStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.cfgname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.tokentype {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.dicts {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_ts_dictionary_stmt(
    value: &pb::AlterTsDictionaryStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.dictname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_type_stmt(
    value: &pb::AlterTypeStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.type_name {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alter_user_mapping_stmt(
    value: &pb::AlterUserMappingStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.user {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_alternative_sub_plan(
    value: &pb::AlternativeSubPlan,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.subplans {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_array_coerce_expr(
    value: &pb::ArrayCoerceExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.elemexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_array_expr(
    value: &pb::ArrayExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.elements {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_bit_string(
    _value: &pb::BitString,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_bool_expr(
    value: &pb::BoolExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_boolean(
    _value: &pb::Boolean,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_boolean_test(
    value: &pb::BooleanTest,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_call_context(
    _value: &pb::CallContext,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_call_stmt(
    value: &pb::CallStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.funccall {
        descend_func_call(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.funcexpr {
        descend_func_expr(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.outargs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_case_expr(
    value: &pb::CaseExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.defresult {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_case_test_expr(
    value: &pb::CaseTestExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_case_when(
    value: &pb::CaseWhen,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.result {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_check_point_stmt(
    _value: &pb::CheckPointStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_close_portal_stmt(
    _value: &pb::ClosePortalStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_cluster_stmt(
    value: &pb::ClusterStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.params {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_coalesce_expr(
    value: &pb::CoalesceExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_coerce_to_domain(
    value: &pb::CoerceToDomain,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_coerce_to_domain_value(
    value: &pb::CoerceToDomainValue,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_coerce_via_io(
    value: &pb::CoerceViaIo,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_collate_clause(
    value: &pb::CollateClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.collname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_collate_expr(
    value: &pb::CollateExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_column_def(
    value: &pb::ColumnDef,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.type_name {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.raw_default {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.cooked_default {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.identity_sequence {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.coll_clause {
        descend_collate_clause(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.constraints {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.fdwoptions {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_column_ref(
    value: &pb::ColumnRef,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.fields {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_comment_stmt(
    value: &pb::CommentStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.object {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_common_table_expr(
    value: &pb::CommonTableExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.aliascolnames {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.ctequery {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.search_clause {
        descend_cte_search_clause(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.cycle_clause {
        descend_cte_cycle_clause(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.ctecolnames {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.ctecoltypes {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.ctecoltypmods {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.ctecolcollations {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_composite_type_stmt(
    value: &pb::CompositeTypeStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.typevar {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.coldeflist {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_constraint(
    value: &pb::Constraint,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    descend_constraint_0(value, depth, scan)?;
    descend_constraint_1(value, depth, scan)?;
    Ok(())
}

fn descend_constraint_0(
    value: &pb::Constraint,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.raw_expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.keys {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.including {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.exclusions {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.where_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.pktable {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.fk_attrs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_constraint_1(
    value: &pb::Constraint,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.pk_attrs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.fk_del_set_cols {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.old_conpfeqop {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_constraints_set_stmt(
    value: &pb::ConstraintsSetStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.constraints {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_convert_rowtype_expr(
    value: &pb::ConvertRowtypeExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_copy_stmt(
    value: &pb::CopyStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.query {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.attlist {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.where_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_am_stmt(
    value: &pb::CreateAmStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.handler_name {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_cast_stmt(
    value: &pb::CreateCastStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.sourcetype {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.targettype {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.func {
        descend_object_with_args(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_conversion_stmt(
    value: &pb::CreateConversionStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.conversion_name {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.func_name {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_domain_stmt(
    value: &pb::CreateDomainStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.domainname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.type_name {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.coll_clause {
        descend_collate_clause(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.constraints {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_enum_stmt(
    value: &pb::CreateEnumStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.type_name {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.vals {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_event_trig_stmt(
    value: &pb::CreateEventTrigStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.whenclause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.funcname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_extension_stmt(
    value: &pb::CreateExtensionStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_fdw_stmt(
    value: &pb::CreateFdwStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.func_options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_foreign_server_stmt(
    value: &pb::CreateForeignServerStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_foreign_table_stmt(
    value: &pb::CreateForeignTableStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.base_stmt {
        descend_create_stmt(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_function_stmt(
    value: &pb::CreateFunctionStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.funcname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.parameters {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.return_type {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.sql_body {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_op_class_item(
    value: &pb::CreateOpClassItem,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.name {
        descend_object_with_args(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.order_family {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.class_args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.storedtype {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_op_class_stmt(
    value: &pb::CreateOpClassStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.opclassname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.opfamilyname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.datatype {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.items {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_op_family_stmt(
    value: &pb::CreateOpFamilyStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.opfamilyname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_p_lang_stmt(
    value: &pb::CreatePLangStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.plhandler {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.plinline {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.plvalidator {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_policy_stmt(
    value: &pb::CreatePolicyStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.table {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.roles {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.qual {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.with_check {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_publication_stmt(
    value: &pb::CreatePublicationStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.pubobjects {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_range_stmt(
    value: &pb::CreateRangeStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.type_name {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.params {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_role_stmt(
    value: &pb::CreateRoleStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_schema_stmt(
    value: &pb::CreateSchemaStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.authrole {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.schema_elts {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_seq_stmt(
    value: &pb::CreateSeqStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.sequence {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_stats_stmt(
    value: &pb::CreateStatsStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.defnames {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.stat_types {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.exprs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.relations {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_stmt(
    value: &pb::CreateStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.table_elts {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.inh_relations {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.partbound {
        descend_partition_bound_spec(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.partspec {
        descend_partition_spec(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.of_typename {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.constraints {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_subscription_stmt(
    value: &pb::CreateSubscriptionStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.publication {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_table_as_stmt(
    value: &pb::CreateTableAsStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.query {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.into {
        descend_into_clause(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_table_space_stmt(
    value: &pb::CreateTableSpaceStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.owner {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_transform_stmt(
    value: &pb::CreateTransformStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.type_name {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.fromsql {
        descend_object_with_args(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.tosql {
        descend_object_with_args(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_trig_stmt(
    value: &pb::CreateTrigStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.funcname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.columns {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.when_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.transition_rels {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.constrrel {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_create_user_mapping_stmt(
    value: &pb::CreateUserMappingStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.user {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_createdb_stmt(
    value: &pb::CreatedbStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_cte_cycle_clause(
    value: &pb::CteCycleClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.cycle_col_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.cycle_mark_value {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.cycle_mark_default {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_cte_search_clause(
    value: &pb::CteSearchClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.search_col_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_current_of_expr(
    value: &pb::CurrentOfExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_deallocate_stmt(
    _value: &pb::DeallocateStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_declare_cursor_stmt(
    value: &pb::DeclareCursorStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.query {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_def_elem(value: &pb::DefElem, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_define_stmt(
    value: &pb::DefineStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.defnames {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.definition {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_delete_stmt(
    value: &pb::DeleteStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.using_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.where_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.returning_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.with_clause {
        descend_with_clause(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_discard_stmt(
    _value: &pb::DiscardStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_distinct_expr(
    value: &pb::DistinctExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_do_stmt(value: &pb::DoStmt, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_drop_owned_stmt(
    value: &pb::DropOwnedStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.roles {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_drop_role_stmt(
    value: &pb::DropRoleStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.roles {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_drop_stmt(
    value: &pb::DropStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.objects {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_drop_subscription_stmt(
    _value: &pb::DropSubscriptionStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_drop_table_space_stmt(
    _value: &pb::DropTableSpaceStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_drop_user_mapping_stmt(
    value: &pb::DropUserMappingStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.user {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_dropdb_stmt(
    value: &pb::DropdbStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_execute_stmt(
    value: &pb::ExecuteStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.params {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_explain_stmt(
    value: &pb::ExplainStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.query {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_fetch_stmt(
    _value: &pb::FetchStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_field_select(
    value: &pb::FieldSelect,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_field_store(
    value: &pb::FieldStore,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.newvals {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.fieldnums {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_float(_value: &pb::Float, _depth: usize, _scan: &mut Scan) -> Result<(), GuardError> {
    Ok(())
}

fn descend_from_expr(
    value: &pb::FromExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.fromlist {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.quals {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_func_call(
    value: &pb::FuncCall,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.funcname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.agg_order {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.agg_filter {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.over {
        descend_window_def(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_func_expr(
    value: &pb::FuncExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_function_parameter(
    value: &pb::FunctionParameter,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.arg_type {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.defexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_grant_role_stmt(
    value: &pb::GrantRoleStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.granted_roles {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.grantee_roles {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.opt {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.grantor {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_grant_stmt(
    value: &pb::GrantStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.objects {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.privileges {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.grantees {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.grantor {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_grouping_func(
    value: &pb::GroupingFunc,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.refs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_grouping_set(
    value: &pb::GroupingSet,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.content {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_import_foreign_schema_stmt(
    value: &pb::ImportForeignSchemaStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.table_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_index_elem(
    value: &pb::IndexElem,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.collation {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.opclass {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.opclassopts {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_index_stmt(
    value: &pb::IndexStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.index_params {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.index_including_params {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.where_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.exclude_op_names {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_infer_clause(
    value: &pb::InferClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.index_elems {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.where_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_inference_elem(
    value: &pb::InferenceElem,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_inline_code_block(
    _value: &pb::InlineCodeBlock,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_insert_stmt(
    value: &pb::InsertStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.cols {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.select_stmt {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.on_conflict_clause {
        descend_on_conflict_clause(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.returning_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.with_clause {
        descend_with_clause(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_int_list(value: &pb::IntList, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    for node in &value.items {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_integer(
    _value: &pb::Integer,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_into_clause(
    value: &pb::IntoClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.rel {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.col_names {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.view_query {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_join_expr(
    value: &pb::JoinExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.larg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.rarg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.using_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.join_using_alias {
        descend_alias(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.quals {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.alias {
        descend_alias(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_agg_constructor(
    value: &pb::JsonAggConstructor,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.output {
        descend_json_output(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.agg_filter {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.agg_order {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.over {
        descend_window_def(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_argument(
    value: &pb::JsonArgument,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.val {
        descend_json_value_expr(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_array_agg(
    value: &pb::JsonArrayAgg,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.constructor {
        descend_json_agg_constructor(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.arg {
        descend_json_value_expr(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_array_constructor(
    value: &pb::JsonArrayConstructor,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.exprs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.output {
        descend_json_output(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_array_query_constructor(
    value: &pb::JsonArrayQueryConstructor,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.query {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.output {
        descend_json_output(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.format {
        descend_json_format(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_behavior(
    value: &pb::JsonBehavior,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_constructor_expr(
    value: &pb::JsonConstructorExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.func {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.coercion {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.returning {
        descend_json_returning(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_expr(
    value: &pb::JsonExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    descend_json_expr_0(value, depth, scan)?;
    descend_json_expr_1(value, depth, scan)?;
    Ok(())
}

fn descend_json_expr_0(
    value: &pb::JsonExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.formatted_expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.format {
        descend_json_format(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.path_spec {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.returning {
        descend_json_returning(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.passing_names {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.passing_values {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.on_empty {
        descend_json_behavior(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_expr_1(
    value: &pb::JsonExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.on_error {
        descend_json_behavior(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_format(
    _value: &pb::JsonFormat,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_json_func_expr(
    value: &pb::JsonFuncExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.context_item {
        descend_json_value_expr(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.pathspec {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.passing {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.output {
        descend_json_output(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.on_empty {
        descend_json_behavior(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.on_error {
        descend_json_behavior(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_is_predicate(
    value: &pb::JsonIsPredicate,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.format {
        descend_json_format(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_key_value(
    value: &pb::JsonKeyValue,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.key {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.value {
        descend_json_value_expr(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_object_agg(
    value: &pb::JsonObjectAgg,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.constructor {
        descend_json_agg_constructor(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.arg {
        descend_json_key_value(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_object_constructor(
    value: &pb::JsonObjectConstructor,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.exprs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.output {
        descend_json_output(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_output(
    value: &pb::JsonOutput,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.type_name {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.returning {
        descend_json_returning(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_parse_expr(
    value: &pb::JsonParseExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.expr {
        descend_json_value_expr(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.output {
        descend_json_output(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_returning(
    value: &pb::JsonReturning,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.format {
        descend_json_format(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_scalar_expr(
    value: &pb::JsonScalarExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.output {
        descend_json_output(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_serialize_expr(
    value: &pb::JsonSerializeExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.expr {
        descend_json_value_expr(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.output {
        descend_json_output(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_table(
    value: &pb::JsonTable,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.context_item {
        descend_json_value_expr(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.pathspec {
        descend_json_table_path_spec(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.passing {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.columns {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.on_error {
        descend_json_behavior(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.alias {
        descend_alias(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_table_column(
    value: &pb::JsonTableColumn,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.type_name {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.pathspec {
        descend_json_table_path_spec(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.format {
        descend_json_format(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.columns {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.on_empty {
        descend_json_behavior(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.on_error {
        descend_json_behavior(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_table_path(
    _value: &pb::JsonTablePath,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_json_table_path_scan(
    value: &pb::JsonTablePathScan,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.plan {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.path {
        descend_json_table_path(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.child {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_table_path_spec(
    value: &pb::JsonTablePathSpec,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.string {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_table_sibling_join(
    value: &pb::JsonTableSiblingJoin,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.plan {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.lplan {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.rplan {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_json_value_expr(
    value: &pb::JsonValueExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.raw_expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.formatted_expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.format {
        descend_json_format(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_list(value: &pb::List, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    for node in &value.items {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_listen_stmt(
    _value: &pb::ListenStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_load_stmt(
    _value: &pb::LoadStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_lock_stmt(
    value: &pb::LockStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.relations {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_locking_clause(
    value: &pb::LockingClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.locked_rels {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_merge_action(
    value: &pb::MergeAction,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.qual {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.target_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.update_colnos {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_merge_stmt(
    value: &pb::MergeStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.source_relation {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.join_condition {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.merge_when_clauses {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.returning_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.with_clause {
        descend_with_clause(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_merge_support_func(
    value: &pb::MergeSupportFunc,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_merge_when_clause(
    value: &pb::MergeWhenClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.condition {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.target_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.values {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_min_max_expr(
    value: &pb::MinMaxExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_multi_assign_ref(
    value: &pb::MultiAssignRef,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.source {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_named_arg_expr(
    value: &pb::NamedArgExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_next_value_expr(
    value: &pb::NextValueExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_notify_stmt(
    _value: &pb::NotifyStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_null_if_expr(
    value: &pb::NullIfExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_null_test(
    value: &pb::NullTest,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_object_with_args(
    value: &pb::ObjectWithArgs,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.objname {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.objargs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.objfuncargs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_oid_list(value: &pb::OidList, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    for node in &value.items {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_on_conflict_clause(
    value: &pb::OnConflictClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.infer {
        descend_infer_clause(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.target_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.where_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_on_conflict_expr(
    value: &pb::OnConflictExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.arbiter_elems {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arbiter_where {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.on_conflict_set {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.on_conflict_where {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.excl_rel_tlist {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_op_expr(value: &pb::OpExpr, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_param(value: &pb::Param, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_param_ref(
    _value: &pb::ParamRef,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_partition_bound_spec(
    value: &pb::PartitionBoundSpec,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.listdatums {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.lowerdatums {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.upperdatums {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_partition_cmd(
    value: &pb::PartitionCmd,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.name {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.bound {
        descend_partition_bound_spec(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_partition_elem(
    value: &pb::PartitionElem,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.collation {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.opclass {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_partition_range_datum(
    value: &pb::PartitionRangeDatum,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.value {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_partition_spec(
    value: &pb::PartitionSpec,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.part_params {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_pl_assign_stmt(
    value: &pb::PlAssignStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.indirection {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.val {
        descend_select_stmt(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_prepare_stmt(
    value: &pb::PrepareStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.argtypes {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.query {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_publication_obj_spec(
    value: &pb::PublicationObjSpec,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.pubtable {
        descend_publication_table(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_publication_table(
    value: &pb::PublicationTable,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.where_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.columns {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_query(value: &pb::Query, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    descend_query_0(value, depth, scan)?;
    descend_query_1(value, depth, scan)?;
    descend_query_2(value, depth, scan)?;
    Ok(())
}

fn descend_query_0(value: &pb::Query, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.utility_stmt {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.cte_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.rtable {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.rteperminfos {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.jointree {
        descend_from_expr(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.merge_action_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.merge_join_condition {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.target_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_query_1(value: &pb::Query, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(item) = &value.on_conflict {
        descend_on_conflict_expr(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.returning_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.group_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.grouping_sets {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.having_qual {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.window_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.distinct_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.sort_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_query_2(value: &pb::Query, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.limit_offset {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.limit_count {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.row_marks {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.set_operations {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.constraint_deps {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.with_check_options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_range_function(
    value: &pb::RangeFunction,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.functions {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.alias {
        descend_alias(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.coldeflist {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_range_subselect(
    value: &pb::RangeSubselect,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.subquery {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.alias {
        descend_alias(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_range_table_func(
    value: &pb::RangeTableFunc,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.docexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.rowexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.namespaces {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.columns {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.alias {
        descend_alias(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_range_table_func_col(
    value: &pb::RangeTableFuncCol,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.type_name {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.colexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.coldefexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_range_table_sample(
    value: &pb::RangeTableSample,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.relation {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.method {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.repeatable {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_range_tbl_entry(
    value: &pb::RangeTblEntry,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    descend_range_tbl_entry_0(value, depth, scan)?;
    descend_range_tbl_entry_1(value, depth, scan)?;
    Ok(())
}

fn descend_range_tbl_entry_0(
    value: &pb::RangeTblEntry,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.alias {
        descend_alias(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.eref {
        descend_alias(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.tablesample {
        descend_table_sample_clause(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.subquery {
        descend_query(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.joinaliasvars {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.joinleftcols {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.joinrightcols {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.join_using_alias {
        descend_alias(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_range_tbl_entry_1(
    value: &pb::RangeTblEntry,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.functions {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.tablefunc {
        descend_table_func(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.values_lists {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.coltypes {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.coltypmods {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.colcollations {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.security_quals {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_range_tbl_function(
    value: &pb::RangeTblFunction,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.funcexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.funccolnames {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.funccoltypes {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.funccoltypmods {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.funccolcollations {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_range_tbl_ref(
    _value: &pb::RangeTblRef,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_range_var(
    value: &pb::RangeVar,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.alias {
        descend_alias(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_raw_stmt(value: &pb::RawStmt, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.stmt {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_reassign_owned_stmt(
    value: &pb::ReassignOwnedStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.roles {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.newrole {
        descend_role_spec(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_refresh_mat_view_stmt(
    value: &pb::RefreshMatViewStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_reindex_stmt(
    value: &pb::ReindexStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.params {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_relabel_type(
    value: &pb::RelabelType,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_rename_stmt(
    value: &pb::RenameStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.object {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_replica_identity_stmt(
    _value: &pb::ReplicaIdentityStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_res_target(
    value: &pb::ResTarget,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.indirection {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.val {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_return_stmt(
    value: &pb::ReturnStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.returnval {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_role_spec(
    _value: &pb::RoleSpec,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_row_compare_expr(
    value: &pb::RowCompareExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.opnos {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.opfamilies {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.inputcollids {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.largs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.rargs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_row_expr(value: &pb::RowExpr, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.colnames {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_row_mark_clause(
    _value: &pb::RowMarkClause,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_rte_permission_info(
    _value: &pb::RtePermissionInfo,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_rule_stmt(
    value: &pb::RuleStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.where_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.actions {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_scalar_array_op_expr(
    value: &pb::ScalarArrayOpExpr,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_sec_label_stmt(
    value: &pb::SecLabelStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.object {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_select_stmt(
    value: &pb::SelectStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    descend_select_stmt_0(value, depth, scan)?;
    descend_select_stmt_1(value, depth, scan)?;
    Ok(())
}

fn descend_select_stmt_0(
    value: &pb::SelectStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.distinct_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.into_clause {
        descend_into_clause(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.target_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.from_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.where_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.group_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.having_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.window_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_select_stmt_1(
    value: &pb::SelectStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.values_lists {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.sort_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.limit_offset {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.limit_count {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.locking_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.with_clause {
        descend_with_clause(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.larg {
        descend_select_stmt(item, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.rarg {
        descend_select_stmt(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_set_operation_stmt(
    value: &pb::SetOperationStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.larg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.rarg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.col_types {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.col_typmods {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.col_collations {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.group_clauses {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_set_to_default(
    value: &pb::SetToDefault,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_single_partition_spec(
    _value: &pb::SinglePartitionSpec,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_sort_by(value: &pb::SortBy, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.node {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.use_op {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_sort_group_clause(
    _value: &pb::SortGroupClause,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_sql_value_function(
    value: &pb::SqlValueFunction,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_stats_elem(
    value: &pb::StatsElem,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_string(_value: &pb::String, _depth: usize, _scan: &mut Scan) -> Result<(), GuardError> {
    Ok(())
}

fn descend_sub_link(value: &pb::SubLink, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.testexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.oper_name {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.subselect {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_sub_plan(value: &pb::SubPlan, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.testexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.param_ids {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.set_param {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.par_param {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_subscripting_ref(
    value: &pb::SubscriptingRef,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.refupperindexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.reflowerindexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.refexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.refassgnexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_table_func(
    value: &pb::TableFunc,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    descend_table_func_0(value, depth, scan)?;
    descend_table_func_1(value, depth, scan)?;
    Ok(())
}

fn descend_table_func_0(
    value: &pb::TableFunc,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.ns_uris {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.ns_names {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.docexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.rowexpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.colnames {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.coltypes {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.coltypmods {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.colcollations {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_table_func_1(
    value: &pb::TableFunc,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.colexprs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.coldefexprs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.colvalexprs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.passingvalexprs {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.plan {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_table_like_clause(
    value: &pb::TableLikeClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_table_sample_clause(
    value: &pb::TableSampleClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.repeatable {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_target_entry(
    value: &pb::TargetEntry,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_transaction_stmt(
    value: &pb::TransactionStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_trigger_transition(
    _value: &pb::TriggerTransition,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_truncate_stmt(
    value: &pb::TruncateStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.relations {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_type_cast(
    value: &pb::TypeCast,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.type_name {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_type_name(
    value: &pb::TypeName,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.names {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.typmods {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.array_bounds {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_unlisten_stmt(
    _value: &pb::UnlistenStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_update_stmt(
    value: &pb::UpdateStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.target_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.where_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.from_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.returning_list {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.with_clause {
        descend_with_clause(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_vacuum_relation(
    value: &pb::VacuumRelation,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.relation {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.va_cols {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_vacuum_stmt(
    value: &pb::VacuumStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.rels {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_var(value: &pb::Var, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_variable_set_stmt(
    value: &pb::VariableSetStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_variable_show_stmt(
    _value: &pb::VariableShowStmt,
    _depth: usize,
    _scan: &mut Scan,
) -> Result<(), GuardError> {
    Ok(())
}

fn descend_view_stmt(
    value: &pb::ViewStmt,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(item) = &value.view {
        descend_range_var(item, depth.saturating_add(1), scan)?;
    }
    for node in &value.aliases {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.query {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.options {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_window_clause(
    value: &pb::WindowClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.partition_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.order_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.start_offset {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.end_offset {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_window_def(
    value: &pb::WindowDef,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.partition_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.order_clause {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.start_offset {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.end_offset {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_window_func(
    value: &pb::WindowFunc,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.aggfilter {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.run_condition {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_window_func_run_condition(
    value: &pb::WindowFuncRunCondition,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(node) = &value.arg {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_with_check_option(
    value: &pb::WithCheckOption,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.qual {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_with_clause(
    value: &pb::WithClause,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    for node in &value.ctes {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_xml_expr(value: &pb::XmlExpr, depth: usize, scan: &mut Scan) -> Result<(), GuardError> {
    if let Some(node) = &value.xpr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.named_args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.arg_names {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    for node in &value.args {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    Ok(())
}

fn descend_xml_serialize(
    value: &pb::XmlSerialize,
    depth: usize,
    scan: &mut Scan,
) -> Result<(), GuardError> {
    if let Some(node) = &value.expr {
        descend_node(node, depth.saturating_add(1), scan)?;
    }
    if let Some(item) = &value.type_name {
        descend_type_name(item, depth.saturating_add(1), scan)?;
    }
    Ok(())
}
