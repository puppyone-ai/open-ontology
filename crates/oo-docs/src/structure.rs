//! The structure layer of the ontology (D93).
//!
//! The ontology has two layers that mirror a database: a structure layer (the classes of record files with their
//! properties, types and relations, like table schemas and foreign keys) and an instance layer (what the files say,
//! like rows). The structure layer of documents is induced the way a database column is profiled:
//!
//! * [`observe`]: the fields one file carries (frontmatter keys, table headers, fixed section headings, `key: value`
//!   lines, the directory), with their values. Pure, stored once per content hash.
//! * [`induce`]: files of one record class share fields; a field that recurs is a property of the class. The values
//!   decide its type ([`vtype`]: date, amount, person, project, enum, ...) and a value that names an entity of another
//!   class makes it a relation.
//! * [`store`]: the schema objects, versioned, with the person's decisions (accept, rename, merge, reject).
//! * [`template`]: a record file of a class with a confirmed structure is extracted against it; facts outside the
//!   structure do not enter the ontology.
//! * [`yaml`]: the ontology YAML of both sides (documents and database) with the cross-links between them.
//!
//! Pure parts first: nothing here reads a database until [`store`].

pub mod align;
pub mod dbside;
pub mod induce;
pub mod observe;
pub mod store;
pub mod tablefill;
pub mod template;
pub mod vtype;
pub mod yaml;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

/// Protocol of the stored and exported structure documents.
pub const PROTOCOL: &str = "oo.docs.structure/v1";
/// Version of the observation format (`structure_observation.obs_version`).
pub const OBS_VERSION: &str = "structure-obs/1";
/// Files of a document class before it is a class; a table class needs one file with [`MIN_ROWS`] rows.
pub const MIN_FILES: usize = 3;
/// Rows of a table before it is a class.
pub const MIN_ROWS: usize = 3;
/// Share of the files of a class that carry a field before it is a property.
pub const MIN_COVERAGE: f64 = 0.4;
/// Files that carry a field before it is a property.
pub const MIN_FIELD_FILES: usize = 2;
/// Share of the files from which a property counts as required.
pub const REQUIRED_COVERAGE: f64 = 0.9;
/// Files in which a field that is not in a confirmed class recurs before it is proposed (type promotion).
pub const PROPOSE_FILES: usize = 3;

/// Where in a file a field is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldSource {
    Frontmatter,
    /// A table header.
    Column,
    /// A fixed section heading.
    Section,
    /// A `key: value` line.
    Line,
    /// A varying directory level.
    Directory,
}

impl FieldSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Frontmatter => "frontmatter",
            Self::Column => "column",
            Self::Section => "section",
            Self::Line => "line",
            Self::Directory => "directory",
        }
    }
}

/// How a class is laid out: one record per file, or one record per table row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Document,
    Table,
}

impl Shape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Table => "table",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "document" => Some(Self::Document),
            "table" => Some(Self::Table),
            _ => None,
        }
    }
}

/// Type of the values of a property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Date,
    Amount,
    Number,
    Percent,
    Email,
    Url,
    Id,
    /// A release or format version (`v1.2.3`, `2.0`).
    Version,
    Person,
    Project,
    Organization,
    Enum,
    Text,
    LongText,
}

impl ValueType {
    pub const ALL: [ValueType; 14] = [
        Self::Date,
        Self::Amount,
        Self::Number,
        Self::Percent,
        Self::Email,
        Self::Url,
        Self::Id,
        Self::Version,
        Self::Person,
        Self::Project,
        Self::Organization,
        Self::Enum,
        Self::Text,
        Self::LongText,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Date => "date",
            Self::Amount => "amount",
            Self::Number => "number",
            Self::Percent => "percent",
            Self::Email => "email",
            Self::Url => "url",
            Self::Id => "id",
            Self::Version => "version",
            Self::Person => "person",
            Self::Project => "project",
            Self::Organization => "organization",
            Self::Enum => "enum",
            Self::Text => "text",
            Self::LongText => "long_text",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }
    /// The entity type a value of this type names (it makes the property a relation when values resolve).
    pub const fn entity_type(self) -> Option<&'static str> {
        match self {
            Self::Person => Some("Person"),
            Self::Project => Some("Project"),
            Self::Organization => Some("Organization"),
            _ => None,
        }
    }
}

/// Review state of a property or a class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Proposed,
    Confirmed,
    Rejected,
}

impl Status {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Confirmed => "confirmed",
            Self::Rejected => "rejected",
        }
    }
}

/// What a relation property points at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    /// An entity type (`Person`) or the `class_key` of another structure class.
    pub target: String,
    /// `entity_type` or `class`.
    pub target_kind: String,
    /// Share of the distinct values that name an entity or a record of the target.
    pub resolved: f64,
    /// The core predicate a template writes the relation with.
    pub predicate: String,
}

/// One property of a class: a field that recurs across its files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Property {
    /// Stable ASCII name (`attr.<slug>` in the instance layer).
    pub slug: String,
    /// What the person sees; renaming changes this, never the slug.
    pub name: String,
    /// Canonical keys of the fields it reads (its own and the ones merged into it).
    pub keys: Vec<String>,
    pub source: FieldSource,
    pub value_type: ValueType,
    /// A value holds several items (a list of names).
    pub multi: bool,
    /// Files of the class that carry the field, and the share.
    pub files_with: u32,
    pub coverage: f64,
    /// Share of non-empty values among the records that carry the field.
    pub fill: f64,
    pub required: bool,
    /// The other labels the property is written with (another language, another spelling).
    #[serde(default)]
    pub aliases: Vec<String>,
    pub examples: Vec<String>,
    /// Up to 40 distinct values, to resolve the property against entities and database rows.
    #[serde(default)]
    pub sample_values: Vec<String>,
    /// The values of an enum, most frequent first.
    pub enum_values: Vec<String>,
    pub relation: Option<Relation>,
    pub status: Status,
    /// The person changed the name or merged other properties into it.
    #[serde(default)]
    pub edited: bool,
}

/// One class of record files with its structure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClassStructure {
    pub class_key: String,
    pub label: String,
    /// `meeting_minutes`, `decision_log`, `weekly_report`, `people_table`, `project_table`, `contract` or `other`.
    pub kind: String,
    pub shape: Shape,
    pub status: Status,
    /// Schema version the class was last changed in.
    pub version: u32,
    pub files: u32,
    pub rows: u32,
    pub properties: Vec<Property>,
    /// Up to five member paths.
    pub sample_paths: Vec<String>,
    /// Canonical keys that identify a member (the properties of at least [`MIN_COVERAGE`]).
    pub signature: Vec<String>,
}

impl ClassStructure {
    /// A property by slug.
    pub fn property(&self, slug: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.slug == slug)
    }
    /// The properties that are not rejected (the YAML, the UI and the class signature).
    pub fn live_properties(&self) -> impl Iterator<Item = &Property> {
        self.properties
            .iter()
            .filter(|p| p.status != Status::Rejected)
    }
    /// The slots a template fills: the confirmed properties of a confirmed class. A property that appeared later is a
    /// proposal and fills nothing until a person accepts it.
    pub fn slots(&self) -> impl Iterator<Item = &Property> {
        let on = self.status == Status::Confirmed;
        self.properties
            .iter()
            .filter(move |p| on && p.status == Status::Confirmed)
    }
}
