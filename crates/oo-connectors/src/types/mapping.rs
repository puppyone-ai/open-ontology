//! Column plans. `SourceColumn.type_support.converted` is ignored.

use oo_core::arrow_meta::ArrowTypeName;
use oo_core::canonical::{LogicalField, LogicalType, ARBITRARY_DECIMAL_SCALE};
use oo_core::TYPE_POLICY_V1;

use crate::model::SourceColumn;
use crate::numeric_parts;

use super::facts::{expected_send, TypeFacts, TypeGraph, OID_NUMERIC};
use super::typmod::numeric_typmod_v1;
use super::{TYPE_CONVERSION_PROTOCOL, NUMERIC_TYPMOD_VERSION};

/// Arrow physical kind. Decimal text is used when a series-60 decimal cannot
/// hold every value of the declared typmod.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArrowPhysical {
    Boolean,
    Int16,
    Int32,
    Int64,
    Float64,
    Utf8,
    Binary,
    Date32,
    Time64Micros,
    TimestampMicrosNaive,
    TimestampMicrosUtc,
    Decimal128 { precision: u8, scale: i8 },
    Decimal256 { precision: u8, scale: i8 },
    /// Exact decimal text. `scale` is `None` when each value carries its own.
    DecimalUtf8 {
        precision: Option<i32>,
        scale: Option<i32>,
    },
    List(Box<ArrowPhysical>),
}

impl ArrowPhysical {
    pub fn series60(&self) -> Result<String, String> {
        self.arrow_name()?.series60()
    }

    fn arrow_name(&self) -> Result<ArrowTypeName, String> {
        Ok(match self {
            Self::Boolean => ArrowTypeName::Boolean,
            Self::Int16 => ArrowTypeName::Int16,
            Self::Int32 => ArrowTypeName::Int32,
            Self::Int64 => ArrowTypeName::Int64,
            Self::Float64 => ArrowTypeName::Float64,
            Self::Utf8 | Self::DecimalUtf8 { .. } => ArrowTypeName::Utf8,
            Self::Binary => ArrowTypeName::Binary,
            Self::Date32 => ArrowTypeName::Date32,
            Self::Time64Micros => ArrowTypeName::Time64Micros,
            Self::TimestampMicrosNaive => ArrowTypeName::TimestampMicrosNaive,
            Self::TimestampMicrosUtc => ArrowTypeName::TimestampMicrosUtc,
            Self::Decimal128 { precision, scale } => {
                ArrowTypeName::decimal128(*precision, *scale)?
            }
            Self::Decimal256 { precision, scale } => {
                ArrowTypeName::decimal256(*precision, *scale)?
            }
            Self::List(item) => ArrowTypeName::list(item.arrow_name()?)?,
        })
    }
}

/// Why a column is not fetched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapDecision {
    Lossless,
    Unsupported(&'static str),
}

/// Spec `ColumnMapping` plus the signed-scale compatibility fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnMapping {
    pub protocol: &'static str,
    pub source_type: String,
    pub arrow_type: String,
    pub pg_type: String,
    pub precision: Option<i32>,
    pub scale: Option<i32>,
    /// Unsigned scale from frozen `numeric_parts`, when the typmod is non-negative.
    pub catalog_v1_numeric_scale: Option<i32>,
    pub numeric_typmod_version: Option<&'static str>,
    pub timezone: Option<String>,
    pub nullable: bool,
    pub type_policy_version: &'static str,
    pub decision: MapDecision,
    pub metadata: Vec<(String, String)>,
}

/// Decode plan for one source column. Built from server facts, not from a flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnPlan {
    pub mapping: ColumnMapping,
    pub attnum: i32,
    pub name: String,
    /// Catalog column oid. A domain keeps the domain oid.
    pub oid: i64,
    /// Builtin, enum, or array oid used to choose a decoder. A domain keeps the base.
    pub decode_oid: i64,
    pub logical: Option<LogicalType>,
    pub arrow: Option<ArrowPhysical>,
    pub element: Option<Box<ColumnPlan>>,
    pub enum_labels: Vec<String>,
    pub key: bool,
}

impl ColumnPlan {
    /// PostgreSQL describes a domain column as its base type.
    /// Any other oid is a mismatch.
    pub(crate) fn accepts_result_oid(&self, actual: i64) -> bool {
        actual == self.oid || (self.decode_oid != self.oid && actual == self.decode_oid)
    }
}

pub fn map_column(column: &SourceColumn, graph: &TypeGraph, key: bool) -> ColumnPlan {
    let _ignored_converted_flag = column.type_support.converted;
    let facts = graph.get(column.type_oid);
    let mut plan = match facts {
        Some(facts) => plan_type(column, facts, graph, column.typmod, 0),
        None => unsupported(column, "type_facts_missing"),
    };
    plan.key = key;
    plan.attnum = column.attnum;
    plan.name = column.name.clone();
    if column.current_privileges.select != Some(true) {
        plan = unsupported(column, "select_not_granted");
        plan.key = key;
        plan.attnum = column.attnum;
        plan.name = column.name.clone();
    }
    plan
}

fn plan_type(
    column: &SourceColumn,
    facts: &TypeFacts,
    graph: &TypeGraph,
    typmod: i32,
    depth: usize,
) -> ColumnPlan {
    if depth > 8 {
        return unsupported(column, "type_depth");
    }
    if facts.typsend_schema != "pg_catalog" {
        return unsupported(column, "user_defined_send");
    }
    if facts.typtype == "d" {
        return plan_domain(column, facts, graph, typmod, depth);
    }
    if facts.typsend_name == "array_send" {
        return plan_array(column, facts, graph, typmod, depth);
    }
    if facts.typtype == "e" && facts.typsend_name == "enum_send" {
        return plan_enum(column, facts, graph);
    }
    if let Some(send) = expected_send(facts.oid)
        && facts.typsend_name == send
        && facts.schema == "pg_catalog"
        && facts.typtype == "b"
    {
        return plan_builtin(column, facts, typmod);
    }
    unsupported(column, reason_for(facts))
}

fn reason_for(facts: &TypeFacts) -> &'static str {
    match facts.typtype.as_str() {
        "c" => "composite",
        "p" => "pseudo",
        "r" => "range",
        "m" => "multirange",
        _ if facts.name == "money" => "money_locale",
        _ if facts.name == "timetz" => "timetz",
        _ => "user_defined_type",
    }
}

fn plan_domain(
    column: &SourceColumn,
    facts: &TypeFacts,
    graph: &TypeGraph,
    typmod: i32,
    depth: usize,
) -> ColumnPlan {
    if depth + 1 > 8 {
        return unsupported(column, "type_depth");
    }
    let Some(base) = graph.get(facts.typbasetype) else {
        return unsupported(column, "domain_base_missing");
    };
    if base.oid == facts.oid {
        return unsupported(column, "domain_cycle");
    }
    let base_mod = if facts.typtypmod >= 0 {
        facts.typtypmod
    } else {
        typmod
    };
    let mut inner = plan_type(column, base, graph, base_mod, depth + 1);
    if inner.mapping.decision != MapDecision::Lossless {
        return inner;
    }
    inner.mapping.source_type = qualified(facts);
    inner.mapping.pg_type = inner
        .mapping
        .metadata
        .iter()
        .find(|(key, _)| key == "oo.pg.base_type")
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| qualified(base));
    inner.oid = facts.oid;
    inner
        .mapping
        .metadata
        .push(("oo.pg.domain".to_string(), qualified(facts)));
    inner
}

fn plan_array(
    column: &SourceColumn,
    facts: &TypeFacts,
    graph: &TypeGraph,
    typmod: i32,
    depth: usize,
) -> ColumnPlan {
    let Some(element_facts) = graph.get(facts.typelem) else {
        return unsupported(column, "array_element_missing");
    };
    if element_facts.oid == facts.oid {
        return unsupported(column, "array_cycle");
    }
    let element = plan_type(column, element_facts, graph, typmod, depth + 1);
    if element.mapping.decision != MapDecision::Lossless {
        return unsupported(column, "array_element_unsupported");
    }
    let Some(element_logical) = element.logical.clone() else {
        return unsupported(column, "array_element_unsupported");
    };
    let Some(element_arrow) = element.arrow.clone() else {
        return unsupported(column, "array_element_unsupported");
    };
    let logical = LogicalType::Struct(vec![
        LogicalField {
            name: "lower_bounds".to_string(),
            logical_type: LogicalType::Array(Box::new(LogicalType::Int)),
            nullable: false,
        },
        LogicalField {
            name: "lengths".to_string(),
            logical_type: LogicalType::Array(Box::new(LogicalType::Int)),
            nullable: false,
        },
        LogicalField {
            name: "elements".to_string(),
            logical_type: LogicalType::Array(Box::new(element_logical)),
            nullable: false,
        },
    ]);
    let arrow = ArrowPhysical::List(Box::new(element_arrow));
    let arrow_type = match arrow.series60() {
        Ok(value) => value,
        Err(_) => return unsupported(column, "arrow_series"),
    };
    let mut mapping = base_mapping(column, &qualified(facts), &arrow_type, &qualified(facts));
    mapping.decision = MapDecision::Lossless;
    mapping.metadata.push((
        "oo.pg.array".to_string(),
        "lower_bounds and lengths are canonical; arrow lists are storage order".to_string(),
    ));
    ColumnPlan {
        mapping,
        attnum: column.attnum,
        name: column.name.clone(),
        oid: facts.oid,
        decode_oid: facts.oid,
        logical: Some(logical),
        arrow: Some(arrow),
        element: Some(Box::new(element)),
        enum_labels: Vec::new(),
        key: false,
    }
}

fn plan_enum(column: &SourceColumn, facts: &TypeFacts, graph: &TypeGraph) -> ColumnPlan {
    let labels = if !graph.enum_labels(facts.oid).is_empty() {
        graph.enum_labels(facts.oid).to_vec()
    } else if column.type_oid == facts.oid {
        column.enum_labels.clone()
    } else {
        Vec::new()
    };
    if labels.is_empty() {
        return unsupported(column, "enum_labels_missing");
    }
    let mut mapping = base_mapping(column, &qualified(facts), "utf8", &qualified(facts));
    mapping.decision = MapDecision::Lossless;
    mapping.metadata.push(("oo.pg.enum".to_string(), "label".to_string()));
    ColumnPlan {
        mapping,
        attnum: column.attnum,
        name: column.name.clone(),
        oid: facts.oid,
        decode_oid: facts.oid,
        logical: Some(LogicalType::Text),
        arrow: Some(ArrowPhysical::Utf8),
        element: None,
        enum_labels: labels,
        key: false,
    }
}

struct BuiltinShape {
    logical: LogicalType,
    arrow: ArrowPhysical,
    precision: Option<i32>,
    scale: Option<i32>,
    timezone: Option<String>,
}

fn shape(
    logical: LogicalType,
    arrow: ArrowPhysical,
    precision: Option<i32>,
    scale: Option<i32>,
    timezone: Option<String>,
) -> BuiltinShape {
    BuiltinShape {
        logical,
        arrow,
        precision,
        scale,
        timezone,
    }
}

fn plan_builtin(column: &SourceColumn, facts: &TypeFacts, typmod: i32) -> ColumnPlan {
    let built = match facts.oid {
        super::facts::OID_BOOL => Some(shape(LogicalType::Bool, ArrowPhysical::Boolean, None, None, None)),
        super::facts::OID_INT2 => Some(shape(LogicalType::Int, ArrowPhysical::Int16, None, None, None)),
        super::facts::OID_INT4 => Some(shape(LogicalType::Int, ArrowPhysical::Int32, None, None, None)),
        super::facts::OID_INT8 => Some(shape(LogicalType::Int, ArrowPhysical::Int64, None, None, None)),
        super::facts::OID_FLOAT4 | super::facts::OID_FLOAT8 => {
            Some(shape(LogicalType::Float64, ArrowPhysical::Float64, None, None, None))
        }
        super::facts::OID_TEXT | super::facts::OID_VARCHAR | super::facts::OID_BPCHAR => {
            let precision = char_len(typmod);
            Some(shape(LogicalType::Text, ArrowPhysical::Utf8, precision, None, None))
        }
        super::facts::OID_BYTEA | super::facts::OID_UUID => {
            Some(shape(LogicalType::Bytes, ArrowPhysical::Binary, None, None, None))
        }
        super::facts::OID_JSON => Some(shape(LogicalType::Text, ArrowPhysical::Utf8, None, None, None)),
        super::facts::OID_JSONB => Some(shape(LogicalType::Json, ArrowPhysical::Utf8, None, None, None)),
        super::facts::OID_DATE => Some(shape(LogicalType::Date, ArrowPhysical::Date32, None, None, None)),
        super::facts::OID_TIME => Some(shape(LogicalType::Time, ArrowPhysical::Time64Micros, None, None, None)),
        super::facts::OID_TIMESTAMP => Some(shape(
            LogicalType::TimestampNaive,
            ArrowPhysical::TimestampMicrosNaive,
            None,
            None,
            None,
        )),
        super::facts::OID_TIMESTAMPTZ => Some(shape(
            LogicalType::TimestampTz,
            ArrowPhysical::TimestampMicrosUtc,
            None,
            None,
            Some("UTC".to_string()),
        )),
        super::facts::OID_INTERVAL => interval_plan(),
        OID_NUMERIC => return plan_numeric(column, facts, typmod),
        _ => None,
    };
    let Some(BuiltinShape {
        logical,
        arrow,
        precision,
        scale,
        timezone,
    }) = built
    else {
        return unsupported(column, "user_defined_type");
    };
    let arrow_type = match arrow.series60() {
        Ok(value) => value,
        Err(_) => return unsupported(column, "arrow_series"),
    };
    let mut mapping = base_mapping(column, &qualified(facts), &arrow_type, &qualified(facts));
    mapping.decision = MapDecision::Lossless;
    mapping.precision = precision;
    mapping.scale = scale;
    mapping.timezone = timezone;
    if facts.oid == super::facts::OID_JSON {
        mapping.metadata.push(("oo.pg.json".to_string(), "exact-text".to_string()));
    }
    if facts.oid == super::facts::OID_JSONB {
        mapping
            .metadata
            .push(("oo.pg.jsonb".to_string(), "canonical-json".to_string()));
    }
    if matches!(facts.oid, super::facts::OID_FLOAT4 | super::facts::OID_FLOAT8) {
        mapping.metadata.push((
            "oo.pg.float".to_string(),
            "canonical hash collapses signed zero; arrow bits keep the sign".to_string(),
        ));
    }
    mapping
        .metadata
        .push(("oo.pg.base_type".to_string(), qualified(facts)));
    ColumnPlan {
        mapping,
        attnum: column.attnum,
        name: column.name.clone(),
        oid: facts.oid,
        decode_oid: facts.oid,
        logical: Some(logical),
        arrow: Some(arrow),
        element: None,
        enum_labels: Vec::new(),
        key: false,
    }
}

fn interval_plan() -> Option<BuiltinShape> {
    let logical = LogicalType::Struct(vec![
        LogicalField {
            name: "months".to_string(),
            logical_type: LogicalType::Int,
            nullable: false,
        },
        LogicalField {
            name: "days".to_string(),
            logical_type: LogicalType::Int,
            nullable: false,
        },
        LogicalField {
            name: "microseconds".to_string(),
            logical_type: LogicalType::Int,
            nullable: false,
        },
    ]);
    let arrow = ArrowPhysical::List(Box::new(ArrowPhysical::Int64));
    Some(shape(logical, arrow, None, None, None))
}

fn plan_numeric(column: &SourceColumn, facts: &TypeFacts, typmod: i32) -> ColumnPlan {
    let declared = numeric_typmod_v1(typmod);
    let catalog_scale = numeric_parts(typmod).map(|(_, scale)| scale);
    let (logical, arrow) = match declared {
        None => (
            LogicalType::Decimal {
                precision: 0,
                scale: ARBITRARY_DECIMAL_SCALE,
            },
            ArrowPhysical::DecimalUtf8 {
                precision: None,
                scale: None,
            },
        ),
        Some(parts) => {
            let logical = match (u16::try_from(parts.precision), i16::try_from(parts.scale)) {
                (Ok(precision), Ok(scale)) if precision > 0 => {
                    LogicalType::Decimal { precision, scale }
                }
                _ => return unsupported(column, "numeric_typmod"),
            };
            let arrow = match decimal_arrow(parts.precision, parts.scale) {
                Some(arrow) => arrow,
                None => ArrowPhysical::DecimalUtf8 {
                    precision: Some(parts.precision),
                    scale: Some(parts.scale),
                },
            };
            (logical, arrow)
        }
    };
    let arrow_type = match arrow.series60() {
        Ok(value) => value,
        Err(_) => return unsupported(column, "arrow_series"),
    };
    let mut mapping = base_mapping(column, &qualified(facts), &arrow_type, &qualified(facts));
    mapping.decision = MapDecision::Lossless;
    mapping.precision = declared.map(|parts| parts.precision);
    mapping.scale = declared.map(|parts| parts.scale);
    mapping.catalog_v1_numeric_scale = catalog_scale;
    mapping.numeric_typmod_version = Some(NUMERIC_TYPMOD_VERSION);
    mapping
        .metadata
        .push(("oo.pg.base_type".to_string(), qualified(facts)));
    if let ArrowPhysical::DecimalUtf8 { precision, scale } = &arrow {
        mapping.metadata.push(("oo.pg.decimal".to_string(), "exact".to_string()));
        if let Some(precision) = precision {
            mapping.metadata.push(("oo.pg.precision".to_string(), precision.to_string()));
        }
        if let Some(scale) = scale {
            mapping.metadata.push(("oo.pg.scale".to_string(), scale.to_string()));
        }
    }
    ColumnPlan {
        mapping,
        attnum: column.attnum,
        name: column.name.clone(),
        oid: facts.oid,
        decode_oid: facts.oid,
        logical: Some(logical),
        arrow: Some(arrow),
        element: None,
        enum_labels: Vec::new(),
        key: false,
    }
}

/// Decimal128 holds every 38-digit coefficient. Decimal256 holds every 76-digit
/// coefficient. A positive scale above precision, or a scale outside `i8`,
/// cannot be a series-60 decimal, so those columns use exact decimal text.
fn decimal_arrow(precision: i32, scale: i32) -> Option<ArrowPhysical> {
    let scale_i8 = i8::try_from(scale).ok()?;
    if scale > 0 && scale > precision {
        return None;
    }
    if (1..=38).contains(&precision) {
        let precision = u8::try_from(precision).ok()?;
        return Some(ArrowPhysical::Decimal128 {
            precision,
            scale: scale_i8,
        });
    }
    if (39..=76).contains(&precision) {
        let precision = u8::try_from(precision).ok()?;
        return Some(ArrowPhysical::Decimal256 {
            precision,
            scale: scale_i8,
        });
    }
    None
}

fn char_len(typmod: i32) -> Option<i32> {
    if typmod < 4 {
        None
    } else {
        Some(typmod - 4)
    }
}

fn qualified(facts: &TypeFacts) -> String {
    format!("{}.{}", facts.schema, facts.name)
}

fn base_mapping(column: &SourceColumn, source_type: &str, arrow_type: &str, pg_type: &str) -> ColumnMapping {
    ColumnMapping {
        protocol: TYPE_CONVERSION_PROTOCOL,
        source_type: source_type.to_string(),
        arrow_type: arrow_type.to_string(),
        pg_type: pg_type.to_string(),
        precision: None,
        scale: None,
        catalog_v1_numeric_scale: None,
        numeric_typmod_version: None,
        timezone: None,
        nullable: !column.not_null,
        type_policy_version: TYPE_POLICY_V1,
        decision: MapDecision::Unsupported("user_defined_type"),
        metadata: vec![(
            "oo.conversion".to_string(),
            TYPE_CONVERSION_PROTOCOL.to_string(),
        )],
    }
}

fn unsupported(column: &SourceColumn, reason: &'static str) -> ColumnPlan {
    let mut mapping = base_mapping(
        column,
        &format!("{}.{}", column.type_schema, column.type_name),
        "unsupported",
        &format!("{}.{}", column.type_schema, column.type_name),
    );
    mapping.decision = MapDecision::Unsupported(reason);
    ColumnPlan {
        mapping,
        attnum: column.attnum,
        name: column.name.clone(),
        oid: column.type_oid,
        decode_oid: column.type_oid,
        logical: None,
        arrow: None,
        element: None,
        enum_labels: Vec::new(),
        key: false,
    }
}
