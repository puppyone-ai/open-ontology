//! Server type facts. A type name alone does not authenticate a send function.

use std::collections::BTreeMap;

/// One `pg_type` row plus the send function's schema and name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeFacts {
    pub oid: i64,
    pub name: String,
    pub schema: String,
    pub typtype: String,
    pub typlen: i32,
    pub typelem: i64,
    pub typbasetype: i64,
    /// `pg_type.typtypmod`. `-1` when the type does not declare one.
    pub typtypmod: i32,
    pub typsend_oid: i64,
    pub typsend_name: String,
    pub typsend_schema: String,
}

/// Facts loaded for one read. Lookups are by oid.
#[derive(Clone, Debug, Default)]
pub struct TypeGraph {
    by_oid: BTreeMap<i64, TypeFacts>,
    enum_labels: BTreeMap<i64, Vec<String>>,
}

impl TypeGraph {
    pub fn new(facts: impl IntoIterator<Item = TypeFacts>) -> Self {
        let mut by_oid = BTreeMap::new();
        for fact in facts {
            by_oid.insert(fact.oid, fact);
        }
        Self {
            by_oid,
            enum_labels: BTreeMap::new(),
        }
    }

    pub fn get(&self, oid: i64) -> Option<&TypeFacts> {
        self.by_oid.get(&oid)
    }

    pub fn insert(&mut self, fact: TypeFacts) {
        self.by_oid.insert(fact.oid, fact);
    }

    pub fn set_enum_labels(&mut self, oid: i64, labels: Vec<String>) {
        self.enum_labels.insert(oid, labels);
    }

    pub fn enum_labels(&self, oid: i64) -> &[String] {
        self.enum_labels.get(&oid).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn iter(&self) -> impl Iterator<Item = &TypeFacts> {
        self.by_oid.values()
    }
}

#[cfg(test)]
pub(crate) fn builtin(oid: i64, name: &str, send: &str, typlen: i32) -> TypeFacts {
    TypeFacts {
        oid,
        name: name.to_string(),
        schema: "pg_catalog".to_string(),
        typtype: "b".to_string(),
        typlen,
        typelem: 0,
        typbasetype: 0,
        typtypmod: -1,
        typsend_oid: 0,
        typsend_name: send.to_string(),
        typsend_schema: "pg_catalog".to_string(),
    }
}

pub(crate) const OID_BOOL: i64 = 16;
pub(crate) const OID_BYTEA: i64 = 17;
pub(crate) const OID_INT8: i64 = 20;
pub(crate) const OID_INT2: i64 = 21;
pub(crate) const OID_INT4: i64 = 23;
pub(crate) const OID_TEXT: i64 = 25;
pub(crate) const OID_JSON: i64 = 114;
pub(crate) const OID_FLOAT4: i64 = 700;
pub(crate) const OID_FLOAT8: i64 = 701;
pub(crate) const OID_BPCHAR: i64 = 1042;
pub(crate) const OID_VARCHAR: i64 = 1043;
pub(crate) const OID_DATE: i64 = 1082;
pub(crate) const OID_TIME: i64 = 1083;
pub(crate) const OID_TIMESTAMP: i64 = 1114;
pub(crate) const OID_TIMESTAMPTZ: i64 = 1184;
pub(crate) const OID_INTERVAL: i64 = 1186;
pub(crate) const OID_NUMERIC: i64 = 1700;
pub(crate) const OID_UUID: i64 = 2950;
pub(crate) const OID_JSONB: i64 = 3802;

pub(crate) fn expected_send(oid: i64) -> Option<&'static str> {
    Some(match oid {
        OID_BOOL => "boolsend",
        OID_BYTEA => "byteasend",
        OID_INT8 => "int8send",
        OID_INT2 => "int2send",
        OID_INT4 => "int4send",
        OID_TEXT => "textsend",
        OID_JSON => "json_send",
        OID_FLOAT4 => "float4send",
        OID_FLOAT8 => "float8send",
        OID_BPCHAR => "bpcharsend",
        OID_VARCHAR => "varcharsend",
        OID_DATE => "date_send",
        OID_TIME => "time_send",
        OID_TIMESTAMP => "timestamp_send",
        OID_TIMESTAMPTZ => "timestamptz_send",
        OID_INTERVAL => "interval_send",
        OID_NUMERIC => "numeric_send",
        OID_UUID => "uuid_send",
        OID_JSONB => "jsonb_send",
        _ => return None,
    })
}
